use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    task::ready,
};

use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame};
use rand::RngCore;
use serde::Deserialize;
use tokio::sync::mpsc::{self, Sender};
use uuid::Uuid;

struct StreamingBody {
    rx: mpsc::Receiver<Bytes>,
    state: AppState,
    id: Uuid,
}

impl HttpBody for StreamingBody {
    type Data = Bytes;

    type Error = color_eyre::Report;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let msg = ready!(self.rx.poll_recv(cx));
        std::task::Poll::Ready(msg.map(|bytes| Ok(Frame::data(bytes))))
    }
}

impl Drop for StreamingBody {
    fn drop(&mut self) {
        self.state.remove(self.id);
    }
}

enum SessionState {
    Start,
    Downloading,
    Uploading,
    End,
}

struct SessionData {
    state: SessionState,
    tx: Sender<Bytes>,
}

#[derive(Clone)]
struct AppState {
    conn: Arc<Mutex<HashMap<Uuid, SessionData>>>,
}

impl AppState {
    fn insert(&self, id: Uuid, tx: Sender<Bytes>) {
        self.conn.lock().unwrap().insert(
            id,
            SessionData {
                state: SessionState::Start,
                tx,
            },
        );
    }

    fn start_download(&self, id: Uuid) -> Option<Sender<Bytes>> {
        match self.conn.lock().unwrap().get_mut(&id) {
            Some(SessionData { state, tx }) => {
                *state = SessionState::Downloading;
                Some(tx.clone())
            }
            _ => None,
        }
    }

    fn get_if_download(&self, id: Uuid) -> Option<Sender<Bytes>> {
        match self.conn.lock().unwrap().get(&id) {
            Some(SessionData {
                state: SessionState::Downloading,
                tx,
            }) => Some(tx.clone()),
            _ => None,
        }
    }

    fn remove(&self, id: Uuid) {
        self.conn.lock().unwrap().remove(&id);
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    id: Uuid,
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let (tx, rx) = mpsc::channel(1024);
    state.insert(id, tx.clone());
    let html = IndexTemplate { id };
    let _ = tx.send(Bytes::from(html.render().unwrap())).await;
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        [(header::TRANSFER_ENCODING, "chunked")],
        Body::new(StreamingBody { rx, state, id }),
    )
}

#[derive(Template)]
#[template(path = "start.html")]
struct StartTemplate {
    id: Uuid,
}

async fn start(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    if let Some(tx) = state.start_download(id) {
        let html = StartTemplate { id };
        let _ = tx.send(Bytes::from(html.render().unwrap())).await;
    }
}

#[derive(Template)]
#[template(path = "download.html")]
struct DownloadTemplate {
    id: Uuid,
    next_size: usize,
    counter: usize,
}

#[derive(Deserialize)]
struct DownloadQuery {
    i: usize,
}

struct DownloadBody {
    state: AppState,
    id: Uuid,
    size: usize,
    counter: usize,
    is_end_stream: bool,
}

impl HttpBody for DownloadBody {
    type Data = Bytes;

    type Error = color_eyre::Report;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.is_end_stream {
            std::task::Poll::Ready(None)
        } else {
            self.is_end_stream = true;
            std::task::Poll::Ready(Some(Ok(Frame::data(
                RANDOM_BYTES.get().unwrap().slice(0..self.size),
            ))))
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.size as u64)
    }

    fn is_end_stream(&self) -> bool {
        self.is_end_stream
    }
}

impl Drop for DownloadBody {
    fn drop(&mut self) {
        if let Some(tx) = self.state.get_if_download(self.id) {
            let html = DownloadTemplate {
                id: self.id,
                next_size: match self.size {
                    ..20_000_000 => 20_000_000,
                    20_000_000..50_000_000 => 50_000_000,
                    50_000_000.. => 100_000_000,
                },
                counter: self.counter + 1,
            };
            let _ = tx.try_send(Bytes::from(html.render().unwrap()));
        }
    }
}

async fn download(
    State(state): State<AppState>,
    Path((id, size)): Path<(Uuid, usize)>,
    Query(DownloadQuery { i: counter }): Query<DownloadQuery>,
) -> impl IntoResponse {
    Body::new(DownloadBody {
        state,
        id,
        size,
        counter,
        is_end_stream: false,
    })
}

static RANDOM_BYTES: OnceLock<Bytes> = OnceLock::new();

#[tokio::main]
async fn main() {
    println!("Initializing random data...");
    let mut random_data = vec![0u8; 100_000_000];
    rand::rng().fill_bytes(&mut random_data);
    RANDOM_BYTES
        .set(Bytes::from_static(random_data.leak()))
        .unwrap();

    let app = Router::new()
        .route("/", get(index))
        .route("/empty.jpg", get(async || {}))
        .route("/{id}/start.jpg", get(start))
        .route("/{id}/download/{size}/0.jpg", get(download))
        .with_state(AppState {
            conn: Arc::default(),
        });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
