use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use askama::Template;
use axum::{
    body::Body,
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::info;
use uuid::Uuid;

use crate::{
    download::{DOWNLOAD_START_SIZE, DOWNLOAD_TEST_DURATION, DownloadBody},
    session::AppState,
    templates::{FinishDownloadTemplate, IndexTemplate, ResultsTemplate, StartDownloadTemplate},
    utils::{bps_to_string, calculate_bps},
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
            start_size: DOWNLOAD_START_SIZE,
            timestamp: start.elapsed().as_secs_f64(),
        };
        sender.send(Bytes::from(html.render().unwrap())).await;
        tokio::spawn(async move {
            sleep(Duration::from_secs(DOWNLOAD_TEST_DURATION)).await;
            if let Some((download, latency)) = state.stop_download(id) {
                let html = FinishDownloadTemplate {
                    download,
                    latency,
                    max_upload_size: state.max_upload_size.clone(),
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
    ts: f64,
}

pub(crate) async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(DownloadQuery {
        size,
        i: counter,
        ts: timestamp,
    }): Query<DownloadQuery>,
) -> impl IntoResponse {
    state.measure_download_latency(id, timestamp, counter);
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

pub(crate) async fn upload(mut multipart: Multipart) -> impl IntoResponse {
    let start = Instant::now();
    let mut download = None;
    let mut latency = None;
    let mut file_size = None;
    let mut duration = None;
    while let Ok(Some(mut field)) = multipart.next_field().await {
        match field.name().unwrap() {
            "download" => download = field.text().await.ok(),
            "latency" => latency = field.text().await.ok(),
            "file" => {
                while let Ok(Some(chunk)) = field.chunk().await {
                    file_size = file_size.or(Some(0)).map(|size| size + chunk.len());
                    duration = Some(start.elapsed());
                }
            }
            _ => (),
        }
    }
    if let (Some(file_size), Some(download), Some(latency), Some(duration)) =
        (file_size, download, latency, duration)
    {
        let upload = bps_to_string(calculate_bps(duration, file_size));
        let uri = format!(
            "/results?{}",
            serde_urlencoded::to_string(ResultsQuery {
                download,
                upload,
                latency
            })
            .unwrap()
        );
        Redirect::to(&uri).into_response()
    } else {
        StatusCode::BAD_REQUEST.into_response()
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ResultsQuery {
    download: String,
    upload: String,
    latency: String,
}

pub(crate) async fn results(
    Query(ResultsQuery {
        download,
        upload,
        latency,
    }): Query<ResultsQuery>,
) -> impl IntoResponse {
    Html(
        ResultsTemplate {
            download,
            upload,
            latency,
        }
        .render()
        .unwrap(),
    )
}
