use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use askama::Template;
use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, header},
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
    templates::{FinishDownloadTemplate, IndexTemplate, StartDownloadTemplate},
};

pub(crate) async fn index(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let addr = if let Some(ip) = headers.get("X-Forwarded-For")
        && let Ok(ip_str) = ip.to_str()
        && let Some(first_ip_str) = ip_str.split(',').next()
        && let Ok(ip) = first_ip_str.parse()
    {
        ip
    } else {
        addr.ip().to_canonical()
    };
    info!(%id, %addr, "New connection.");
    let (sender, body) = state.insert(id, addr);
    let html = IndexTemplate { id };
    sender.send(Bytes::from(html.render().unwrap())).await;
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        [(header::TRANSFER_ENCODING, "chunked")],
        Body::new(body),
    )
}

pub(crate) async fn favicon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        include_bytes!("./favicon.png"),
    )
}

pub(crate) async fn start(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Some((sender, start)) = state.start_download(id) {
        let html = StartDownloadTemplate {
            id,
            test_duration: DOWNLOAD_TEST_DURATION,
            timestamp: start.elapsed().as_secs_f64(),
        };
        sender.send(Bytes::from(html.render().unwrap())).await;
        tokio::spawn(async move {
            sleep(Duration::from_secs(DOWNLOAD_TEST_DURATION)).await;
            if let Some((download_speed, download_latency)) = state.stop_download(id) {
                let html = FinishDownloadTemplate {
                    download_speed,
                    download_latency,
                };
                sender.send(Bytes::from(html.render().unwrap())).await;
                state.finish(id).await;
            }
        });
    }
}

#[derive(Deserialize)]
pub(crate) struct DownloadQuery {
    i: usize,
    size: usize,
    ts: Option<f64>,
}

pub(crate) async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(DownloadQuery {
        size,
        i: counter,
        ts,
    }): Query<DownloadQuery>,
) -> impl IntoResponse {
    if let Some(timestamp) = ts {
        state.measure_download_latency(id, timestamp, counter);
    }
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
