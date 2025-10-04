use std::{net::SocketAddr, sync::Arc};

use axum::{Router, routing::get};
use bytes::Bytes;
use color_eyre::eyre::{Context, eyre};
use image::{ExtendedColorType, codecs::bmp::BmpEncoder};
use rand::RngCore;
use tracing::{error, info};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    download::RANDOM_BITMAP,
    routes::{download, index, start},
    session::AppState,
};

mod download;
mod routes;
mod session;
mod templates;

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

    let size = 100_000_000;
    let width = 5_000;
    let height = 5_000;
    if ExtendedColorType::Rgba8.bits_per_pixel() as usize * width as usize * height as usize
        != 8 * size
    {
        error!(size, width, height, "Invalid dimensions");
        return Err(eyre!("Cannot initialize random data (invalid dimensions)"));
    }

    info!(size, width, height, "Initializing random data...");
    let mut random_image = vec![];
    let mut encoder = BmpEncoder::new(&mut random_image);
    let mut random_data = vec![0u8; size];
    rand::rng().fill_bytes(&mut random_data);
    encoder
        .encode(&random_data[..], width, height, ExtendedColorType::Rgba8)
        .wrap_err_with(|| "failed to encode bitmap")?;
    drop(random_data);
    RANDOM_BITMAP
        .set(Bytes::from_static(random_image.leak()))
        .unwrap();

    let app = Router::new()
        .route("/", get(index))
        .route("/empty.jpg", get(async || {}))
        .route("/{id}/start.jpg", get(start))
        .route("/{id}/download.bmp", get(download))
        .with_state(AppState {
            conn: Arc::default(),
        });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .wrap_err_with(|| "failed to listen on port 3000")?;
    info!(address = "http://0.0.0.0:3000", "Starting server...");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .wrap_err_with(|| "server closed")
}
