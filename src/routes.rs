use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use askama::Template;
use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    http::header,
    response::IntoResponse,
};
use bytes::Bytes;
use serde::Deserialize;
use tokio::time::sleep;
use tracing::info;
use uuid::Uuid;

use crate::{
    download::{DOWNLOAD_TEST_DURATION, DownloadBody},
    session::AppState,
    templates::{FinishDownloadTemplate, IndexTemplate, StartTemplate},
};

pub(crate) async fn index(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let id = Uuid::new_v4();
    info!(%id, %addr, "New connection.");
    let (tx, body) = state.insert(id, addr);
    let html = IndexTemplate { id };
    let _ = tx.send(Bytes::from(html.render().unwrap())).await;
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        [(header::TRANSFER_ENCODING, "chunked")],
        Body::new(body),
    )
}

pub(crate) async fn start(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Some(tx) = state.start_download(id) {
        let html = StartTemplate {
            id,
            test_duration: DOWNLOAD_TEST_DURATION,
        };
        let _ = tx.send(Bytes::from(html.render().unwrap())).await;
        tokio::spawn(async move {
            sleep(Duration::from_secs(DOWNLOAD_TEST_DURATION)).await;
            state.stop_download(id);
            let html = FinishDownloadTemplate {};
            let _ = tx.try_send(Bytes::from(html.render().unwrap()));
            state.finish(id);
        });
    }
}

#[derive(Deserialize)]
pub(crate) struct DownloadQuery {
    i: usize,
    size: usize,
}

pub(crate) async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(DownloadQuery { size, i: counter }): Query<DownloadQuery>,
) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/bmp")],
        Body::new(DownloadBody {
            instant: Instant::now(),
            state,
            id,
            size,
            counter,
            is_end_stream: false,
        }),
    )
}
