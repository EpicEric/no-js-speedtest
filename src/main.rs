use std::{
    net::{Ipv6Addr, SocketAddr},
    sync::Arc,
};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use bytes::Bytes;
use color_eyre::eyre::{Context, eyre};
use image::{ExtendedColorType, codecs::bmp::BmpEncoder};
use rand::RngCore;
use tracing::{error, info};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    download::RANDOM_BITMAP,
    routes::{download, favicon, index, privacy, results, start, upload},
    session::AppState,
    utils::bytes_to_string,
};

mod download;
mod routes;
mod session;
mod templates;
mod utils;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .compact()
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .with_filter(
                    tracing_subscriber::EnvFilter::builder()
                        .with_default_directive(tracing::level_filters::LevelFilter::INFO.into())
                        .from_env_lossy(),
                ),
        )
        .with(tracing_error::ErrorLayer::default())
        .try_init()
        .wrap_err_with(|| "failed to initialize tracing")?;

    let image_size: usize = 100_000_000;
    let image_width: u32 = 5_000;
    let image_height: u32 = 5_000;
    let server_port: u16 = 3000;
    let max_upload_size: usize = 200_000_000;

    if ExtendedColorType::Rgba8.bits_per_pixel() as usize
        * image_width as usize
        * image_height as usize
        != 8 * image_size
    {
        error!(image_size, image_width, image_height, "Invalid dimensions");
        return Err(eyre!("Cannot initialize random data (invalid dimensions)"));
    }

    info!(
        image_size,
        image_width, image_height, "Initializing random data..."
    );
    let mut random_image = vec![];
    let mut encoder = BmpEncoder::new(&mut random_image);
    let mut random_data = vec![0u8; image_size];
    rand::rng().fill_bytes(&mut random_data);
    encoder
        .encode(
            &random_data[..],
            image_width,
            image_height,
            ExtendedColorType::Rgba8,
        )
        .wrap_err_with(|| "failed to encode bitmap")?;
    drop(random_data);
    RANDOM_BITMAP
        .set(Bytes::from_static(random_image.leak()))
        .unwrap();

    let app = Router::new()
        .route("/", get(index))
        .route("/privacy", get(privacy))
        .route("/favicon.png", get(favicon))
        .route("/empty.jpg", get(async || {}))
        .route("/{id}/start.jpg", get(start))
        .route("/{id}/download.bmp", get(download))
        .route(
            "/upload",
            post(upload).layer(DefaultBodyLimit::max(max_upload_size)),
        )
        .route("/results", get(results))
        .with_state(AppState {
            conn: Arc::default(),
            max_upload_size: bytes_to_string(max_upload_size),
        });

    let listener = tokio::net::TcpListener::bind((Ipv6Addr::UNSPECIFIED, server_port))
        .await
        .wrap_err_with(|| format!("failed to listen on port {server_port}"))?;
    info!(
        address = format!("http://0.0.0.0:{server_port}"),
        "Starting server..."
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .wrap_err_with(|| "server closed")
}
