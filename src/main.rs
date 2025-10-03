use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    task::ready,
};

use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame};
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

#[derive(Clone)]
struct AppState {
    conn: Arc<Mutex<HashMap<Uuid, Sender<Bytes>>>>,
}

impl AppState {
    fn insert(&self, id: Uuid, tx: Sender<Bytes>) {
        self.conn.lock().unwrap().insert(id, tx);
    }

    fn get(&self, id: Uuid) -> Option<Sender<Bytes>> {
        self.conn.lock().unwrap().get(&id).cloned()
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

#[derive(Template)]
#[template(path = "start.html")]
struct StartTemplate {}

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

async fn start(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    if let Some(tx) = state.get(id) {
        let html = StartTemplate {};
        let _ = tx.send(Bytes::from(html.render().unwrap())).await;
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/{id}/start.jpg", get(start))
        .with_state(AppState {
            conn: Arc::default(),
        });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
