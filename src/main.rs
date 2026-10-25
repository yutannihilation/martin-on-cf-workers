use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use martin_core::tiles::{Source as _, UrlQuery};
use martin_core::CacheZoomRange;
use martin_tile_utils::TileCoord;
use object_store::{ObjectStoreExt as _, PutPayload};
use serde::Deserialize;
use tower_service::Service;
use worker::*;

const WEBP2_PMTILES: &[u8] = include_bytes!("../tests/fixtures/pmtiles2/webp2.pmtiles");

fn main() {}

#[derive(Deserialize)]
struct TileRequest {
    z: u8,
    x: u32,
    y: u32,
}

async fn get_tile(
    Path(path): Path<TileRequest>,
    State(state): State<PmtilesSource>,
) -> std::result::Result<Response, (StatusCode, String)> {
    let xyz = TileCoord {
        z: path.z,
        x: path.x,
        y: path.y,
    };

    let tile_data = state
        .get_tile(xyz, Option::<&UrlQuery>::None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if tile_data.is_empty() {
        Ok(StatusCode::NOT_FOUND.into_response())
    } else {
        Ok(([(header::CONTENT_TYPE, "image/webp")], tile_data).into_response())
    }
}

async fn get_style(
    uri: Uri,
    headers: HeaderMap,
    State(state): State<PmtilesSource>,
) -> Json<serde_json::Value> {
    let origin = match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => format!("{scheme}://{authority}"),
        _ => headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .map(|host| format!("http://{host}"))
            .unwrap_or_else(|| "http://localhost:8787".to_owned()),
    };

    let tilejson = state.get_tilejson();

    let style = serde_json::json!({
        "version": 8,
        "name": "PMTiles Example",
        "projection": {
            "type": "globe"
        },
        "sources": {
            "pmtiles": {
                "type": "raster",
                "tiles": [format!("{origin}/tiles/{{z}}/{{x}}/{{y}}")],
                "minzoom": tilejson.minzoom.unwrap_or(0),
                "maxzoom": tilejson.maxzoom.unwrap_or(5),
                "bounds": tilejson.bounds.as_ref().map_or(
                    [-180.0, -85.0511, 180.0, 85.0511],
                    |b| [b.left, b.bottom, b.right, b.top],
                ),
                "tileSize": 512
            }
        },
        "layers": [
            {
                "id": "raster",
                "type": "raster",
                "source": "pmtiles"
            }
        ]
    });

    Json(style)
}

async fn index() -> Html<&'static str> {
    Html(include_str!("pmtiles-tileserver.html"))
}

async fn init_source() -> PmtilesSource {
    let url = url::Url::parse("memory:///webp2.pmtiles").expect("Failed to parse memory URL");
    let (store, path) = object_store::parse_url_opts(&url, std::iter::empty::<(&str, &str)>())
        .expect("Failed to parse object store URL");

    store
        .put(&path, PutPayload::from_static(WEBP2_PMTILES))
        .await
        .expect("Failed to load PMTiles fixture into memory store");

    let cache = PmtCacheInstance::new(0, PmtCache::default());
    PmtilesSource::new(
        cache,
        "webp2".to_owned(),
        store,
        path,
        CacheZoomRange::default(),
    )
    .await
    .expect("Failed to create PMTiles source")
}

fn router(source: PmtilesSource) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/style.json", get(get_style))
        .route("/tiles/{z}/{x}/{y}", get(get_tile))
        .with_state(source)
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    _env: Env,
    _ctx: Context,
) -> Result<http::Response<axum::body::Body>> {
    let source = init_source().await;
    Ok(router(source).call(req).await?)
}
