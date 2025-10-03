use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    task::ready,
    time::Duration,
};

use askama::Template;
use axum::{
    Router, body::Body, extract::State, http::header, response::IntoResponse, routing::get,
};
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame};
use tokio::{
    sync::mpsc::{self, Sender},
    time::sleep,
};
use uuid::Uuid;

struct StreamingBody {
    id: Uuid,
    rx: mpsc::Receiver<Bytes>,
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

#[derive(Clone)]
struct AppState {
    conn: Arc<Mutex<HashMap<Uuid, Sender<Bytes>>>>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    id: Uuid,
}

#[derive(Template)]
#[template(path = "item.html")]
struct ItemTemplate {
    value: usize,
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let (tx, rx) = mpsc::channel(1024);
    state.conn.lock().unwrap().insert(id, tx.clone());
    tokio::spawn(async move {
        let html = IndexTemplate { id };
        tx.send(Bytes::from(html.render().unwrap())).await.unwrap();
        let mut value = 1;
        loop {
            let html = ItemTemplate { value };
            let Ok(_) = tx.send(Bytes::from(html.render().unwrap())).await else {
                break;
            };
            value += 1;
            sleep(Duration::from_millis(100)).await;
        }
    });
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        [(header::TRANSFER_ENCODING, "chunked")],
        Body::new(StreamingBody { id, rx }),
    )
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/", get(index)).with_state(AppState {
        conn: Arc::default(),
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
