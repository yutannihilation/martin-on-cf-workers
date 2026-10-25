use std::fmt::{self, Debug, Display, Formatter};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream::{BoxStream, StreamExt};
use object_store::path::Path;
use object_store::{
    Attributes, CopyOptions, Error, GetOptions, GetRange, GetResult, GetResultPayload, ListResult,
    MultipartUpload, ObjectMeta, ObjectStore, PutMultipartOptions, PutOptions, PutPayload,
    PutResult, Result,
};
use worker::send::{SendFuture, SendWrapper};
use worker::{Bucket, Object, Range};

const STORE_NAME: &str = "R2ObjectStore";

#[derive(Clone)]
pub struct R2ObjectStore {
    bucket: Arc<SendWrapper<Bucket>>,
}

impl R2ObjectStore {
    pub fn new(bucket: Bucket) -> Self {
        Self {
            bucket: Arc::new(SendWrapper::new(bucket)),
        }
    }
}

impl Debug for R2ObjectStore {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct(STORE_NAME).finish_non_exhaustive()
    }
}

impl Display for R2ObjectStore {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{STORE_NAME}")
    }
}

fn r2_error(err: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>) -> Error {
    Error::Generic {
        store: STORE_NAME,
        source: err.into(),
    }
}

fn to_object_meta(obj: &Object, fallback_location: Option<&Path>) -> Result<ObjectMeta> {
    let location = match fallback_location {
        Some(loc) => loc.clone(),
        None => Path::parse(obj.key()).map_err(r2_error)?,
    };
    let uploaded_ms = obj.uploaded().as_millis() as i64;
    let last_modified = DateTime::<Utc>::from_timestamp_millis(uploaded_ms).unwrap_or_default();

    Ok(ObjectMeta {
        location,
        last_modified,
        size: obj.size(),
        e_tag: Some(obj.http_etag()),
        version: Some(obj.version()),
    })
}

fn format_prefix(prefix: Option<&Path>) -> Option<String> {
    prefix
        .map( Path::as_ref)
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}/"))
}

#[async_trait]
impl ObjectStore for R2ObjectStore {
    async fn put_opts(
        &self,
        _location: &Path,
        _payload: PutPayload,
        _opts: PutOptions,
    ) -> Result<PutResult> {
        Err(Error::NotImplemented {
            operation: "put_opts".to_owned(),
            implementer: STORE_NAME.to_owned(),
        })
    }

    async fn put_multipart_opts(
        &self,
        _location: &Path,
        _opts: PutMultipartOptions,
    ) -> Result<Box<dyn MultipartUpload>> {
        Err(Error::NotImplemented {
            operation: "put_multipart_opts".to_owned(),
            implementer: STORE_NAME.to_owned(),
        })
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> Result<GetResult> {
        if let Some(range) = &options.range {
            range.is_valid().map_err(r2_error)?;
        }

        let bucket = Arc::clone(&self.bucket);
        let location = location.clone();

        SendFuture::new(async move {
            let key = location.as_ref();

            if options.head {
                let obj = bucket
                    .head(key)
                    .await
                    .map_err(r2_error)?
                    .ok_or_else(|| Error::NotFound {
                        path: key.to_owned(),
                        source: "Object not found in R2".into(),
                    })?;

                let meta = to_object_meta(&obj, Some(&location))?;
                options.check_preconditions(&meta)?;

                let stream = futures_util::stream::empty().boxed();
                return Ok(GetResult {
                    payload: GetResultPayload::Stream(stream),
                    meta,
                    range: 0..0,
                    attributes: Attributes::default(),
                    extensions: Default::default(),
                });
            }

            let mut get_req = bucket.get(key);
            if let Some(range) = &options.range {
                let r2_range = match range {
                    GetRange::Bounded(r) => Range::OffsetWithLength {
                        offset: r.start,
                        length: r.end - r.start,
                    },
                    GetRange::Offset(offset) => Range::OffsetToEnd { offset: *offset },
                    GetRange::Suffix(suffix) => Range::Suffix { suffix: *suffix },
                };
                get_req = get_req.range(r2_range);
            }

            let obj = get_req
                .execute()
                .await
                .map_err(r2_error)?
                .ok_or_else(|| Error::NotFound {
                    path: key.to_owned(),
                    source: "Object not found in R2".into(),
                })?;

            let meta = to_object_meta(&obj, Some(&location))?;
            options.check_preconditions(&meta)?;

            let range = match &options.range {
                Some(r) => r.as_range(meta.size).map_err(r2_error)?,
                None => 0..meta.size,
            };

            let bytes = match obj.body() {
                Some(body) => Bytes::from(body.bytes().await.map_err(r2_error)?),
                None => Bytes::new(),
            };

            let stream =
                futures_util::stream::once(futures_util::future::ready(Ok(bytes))).boxed();

            Ok(GetResult {
                payload: GetResultPayload::Stream(stream),
                meta,
                range,
                attributes: Attributes::default(),
                extensions: Default::default(),
            })
        })
        .await
    }

    fn delete_stream(
        &self,
        _locations: BoxStream<'static, Result<Path>>,
    ) -> BoxStream<'static, Result<Path>> {
        futures_util::stream::once(futures_util::future::ready(Err(Error::NotImplemented {
            operation: "delete_stream".to_owned(),
            implementer: STORE_NAME.to_owned(),
        })))
        .boxed()
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, Result<ObjectMeta>> {
        let bucket = Arc::clone(&self.bucket);
        let prefix_str = format_prefix(prefix);

        let fut = SendFuture::new(async move {
            let mut metas = Vec::new();
            let mut cursor: Option<String> = None;

            loop {
                let mut req = bucket.list();
                if let Some(p) = &prefix_str {
                    req = req.prefix(p);
                }
                if let Some(c) = &cursor {
                    req = req.cursor(c);
                }

                let objects = req.execute().await.map_err(r2_error)?;
                for obj in objects.objects() {
                    metas.push(Ok(to_object_meta(&obj, None)?));
                }

                if objects.truncated() {
                    cursor = objects.cursor();
                    if cursor.is_none() {
                        break;
                    }
                } else {
                    break;
                }
            }

            Ok::<_, Error>(futures_util::stream::iter(metas))
        });

        futures_util::stream::once(fut)
            .flat_map(|res| match res {
                Ok(stream) => stream.boxed(),
                Err(e) => futures_util::stream::once(futures_util::future::ready(Err(e))).boxed(),
            })
            .boxed()
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> Result<ListResult> {
        let bucket = Arc::clone(&self.bucket);
        let prefix_str = format_prefix(prefix);

        SendFuture::new(async move {
            let mut objects = Vec::new();
            let mut common_prefixes = Vec::new();
            let mut cursor: Option<String> = None;

            loop {
                let mut req = bucket.list().delimiter("/");
                if let Some(p) = &prefix_str {
                    req = req.prefix(p);
                }
                if let Some(c) = &cursor {
                    req = req.cursor(c);
                }

                let res = req.execute().await.map_err(r2_error)?;
                for obj in res.objects() {
                    objects.push(to_object_meta(&obj, None)?);
                }
                for delimited in res.delimited_prefixes() {
                    common_prefixes.push(Path::parse(delimited).map_err(r2_error)?);
                }

                if res.truncated() {
                    cursor = res.cursor();
                    if cursor.is_none() {
                        break;
                    }
                } else {
                    break;
                }
            }

            Ok(ListResult {
                objects,
                common_prefixes,
                extensions: Default::default(),
            })
        })
        .await
    }

    async fn copy_opts(&self, _from: &Path, _to: &Path, _options: CopyOptions) -> Result<()> {
        Err(Error::NotImplemented {
            operation: "copy_opts".to_owned(),
            implementer: STORE_NAME.to_owned(),
        })
    }
}
