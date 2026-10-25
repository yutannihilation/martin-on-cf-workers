mod r2;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use martin_core::tiles::{Source as _, UrlQuery};
use martin_core::CacheZoomRange;
use martin_tile_utils::TileCoord;
use serde::Deserialize;
use tower_service::Service;
use worker::*;

use crate::r2::R2ObjectStore;

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

async fn init_source(bucket: Bucket) -> Result<PmtilesSource> {
    let store = Box::new(R2ObjectStore::new(bucket));
    let cache = PmtCacheInstance::new(0, PmtCache::default());
    PmtilesSource::new(
        cache,
        "webp2".to_owned(),
        store,
        "webp2.pmtiles",
        CacheZoomRange::default(),
    )
    .await
    .map_err(|e| Error::RustError(format!("Failed to create PMTiles source: {e}")))
}

fn router(source: PmtilesSource) -> Router {
    Router::new()
        .route("/style.json", get(get_style))
        .route("/tiles/{z}/{x}/{y}", get(get_tile))
        .with_state(source)
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<http::Response<axum::body::Body>> {
    let bucket = env.bucket("BUCKET")?;
    let source = init_source(bucket).await?;
    Ok(router(source).call(req).await?)
}
