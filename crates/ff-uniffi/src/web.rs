//! Answers the shared web UI's cycle-data requests in-process, for the
//! Android app's WebView (which runs the same React UI as the website).
//!
//! The WebView's request interceptor hands `/data/*` and `/cycles/latest`
//! GETs here instead of to the network. They go through `ff-data-api`'s
//! router — the very code `ff-api` serves the website with — but against
//! the bundle installed on this device, so the UI works offline and gets
//! byte-identical JSON. Still no sockets: the router is called directly as
//! a `tower` service.
//!
//! Chart tiles are not served from here. `/data/charts` rewrites each
//! chart's `tile_url` to `/local-charts/<id>.pmtiles` when its archive is
//! installed (Kotlin serves those byte ranges from [`Freeflight::chart_file`])
//! and to the server's absolute URL otherwise, so charts that haven't been
//! downloaded still stream when there is a connection.

use crate::{query, Freeflight};
use axum::body::Body;
use axum::http::Request;
use std::path::PathBuf;
use tower::ServiceExt;

/// One HTTP response for the WebView.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WebResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Path prefix, on the app's own origin, under which installed charts are
/// served by the Kotlin interceptor.
const LOCAL_CHARTS_PREFIX: &str = "/local-charts/";

#[uniffi::export]
impl Freeflight {
    /// Answer a GET for `path_and_query` (e.g. `/data/search?q=KSFO`).
    ///
    /// `remote_base` is the server (`https://freeflight.butr.cc`), used only
    /// to point not-yet-downloaded charts at their online copy. Never
    /// fails: errors come back as HTTP statuses, as they would from
    /// `ff-api`, so the UI's existing error handling applies unchanged.
    pub fn handle_web_request(&self, path_and_query: String, remote_base: String) -> WebResponse {
        if path_and_query == "/cycles/latest" || path_and_query.starts_with("/cycles/latest?") {
            return self.local_cycle_manifest();
        }
        let runtime = self.web_runtime.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("ff-web")
                .build()
                .expect("tokio runtime for in-process web requests")
        });
        let router = ff_data_api::router::<()>(self.data_state(remote_base));
        runtime.block_on(async move {
            let request = match Request::builder().method("GET").uri(&path_and_query).body(Body::empty()) {
                Ok(request) => request,
                Err(e) => return text(400, format!("bad request URI {path_and_query}: {e}")),
            };
            let response = match router.oneshot(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            };
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            match axum::body::to_bytes(response.into_body(), usize::MAX).await {
                Ok(body) => WebResponse {
                    status,
                    content_type,
                    body: body.to_vec(),
                },
                Err(e) => text(500, e.to_string()),
            }
        })
    }

    /// The installed PMTiles archive for `chart_id`, or `None` if it isn't
    /// on the device (or there is no cycle).
    pub fn chart_file(&self, chart_id: String) -> Option<String> {
        let hash = self
            .with_db(|conn| query::chart_hash(conn, &chart_id))
            .ok()?
            .0;
        let path = self.layout.chart_blob_path(&hash)?;
        path.is_file().then(|| path.display().to_string())
    }
}

impl Freeflight {
    fn data_state(&self, remote_base: String) -> ff_data_api::DataState {
        let bundle_layout = self.layout.clone();
        let chart_layout = self.layout.clone();
        let remote_base = remote_base.trim_end_matches('/').to_string();
        ff_data_api::DataState::new(move || {
            Ok(bundle_layout
                .current_cycle_id()
                .map(|cycle_id| bundle_layout.bundle_path(&cycle_id))
                .filter(|path: &PathBuf| path.is_file()))
        })
        .with_tile_url(move |conn, chart| {
            let installed = query::chart_hash(conn, &chart.id)
                .ok()
                .and_then(|(hash, _)| chart_layout.chart_blob_path(&hash))
                .is_some_and(|path| path.is_file());
            if installed {
                format!("{LOCAL_CHARTS_PREFIX}{}.pmtiles", chart.id)
            } else if chart.tile_url.starts_with('/') {
                format!("{remote_base}{}", chart.tile_url)
            } else {
                chart.tile_url.clone()
            }
        })
    }

    /// What `/cycles/latest` means on the device: the installed cycle. The
    /// UI only reads `cycle_id` from it; there is nothing to download, so
    /// the URL and checksum (verified when the bundle was installed) are
    /// left empty rather than invented.
    fn local_cycle_manifest(&self) -> WebResponse {
        match self.layout.current_cycle_id() {
            Some(cycle_id) => WebResponse {
                status: 200,
                content_type: "application/json".to_string(),
                body: serde_json::json!({
                    "cycle_id": cycle_id,
                    "sqlite_url": "",
                    "sqlite_sha256": "",
                    "pmtiles_url": null,
                    "pmtiles_sha256": null,
                })
                .to_string()
                .into_bytes(),
            },
            None => text(501, "no cycle installed on this device yet".to_string()),
        }
    }
}

fn text(status: u16, message: String) -> WebResponse {
    WebResponse {
        status,
        content_type: "text/plain; charset=utf-8".to_string(),
        body: message.into_bytes(),
    }
}
