use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
    time::Instant,
};

use ahash::RandomState;
use bytes::Bytes;
use dashmap::DashMap;
use http_body::{Body as HttpBody, Frame};
use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

pub(crate) struct StreamingBody {
    rx: mpsc::Receiver<Bytes>,
    state: AppState,
    id: Uuid,
    addr: SocketAddr,
}

impl HttpBody for StreamingBody {
    type Data = Bytes;

    type Error = color_eyre::Report;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let msg = ready!(self.rx.poll_recv(cx));
        Poll::Ready(msg.and_then(|bytes| {
            if bytes.is_empty() {
                None
            } else {
                Some(Ok(Frame::data(bytes)))
            }
        }))
    }
}

impl Drop for StreamingBody {
    fn drop(&mut self) {
        info!(id = %self.id, addr = %self.addr, "Disconnecting.");
        self.state.remove(self.id);
    }
}

pub(crate) enum SessionState {
    Start,
    Downloading {
        start: Instant,
        average: f64,
        total_weights: f64,
    },
    End {
        _download: String,
    },
}

pub(crate) struct SessionData {
    state: SessionState,
    tx: mpsc::Sender<Bytes>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) conn: Arc<DashMap<Uuid, SessionData, RandomState>>,
}

fn calculate_bps(instant: Instant, size: usize) -> f64 {
    (size as f64 / instant.elapsed().as_secs_f64()) * 8.0
}

fn calculate_weight(start: Instant, size: usize) -> f64 {
    ((size / 5_000_000) as f64) * start.elapsed().as_secs_f64()
}

static SPEED_SUFFIX: [&str; 9] = [
    " bps", " Kbps", " Mbps", " Gbps", " Tbps", " Pbps", " Ebps", " Zbps", " Ybps",
];

fn bps_to_string(mut speed: f64) -> String {
    let mut order_of_magnitude = 0;
    while speed > 1_000.0 {
        order_of_magnitude += 1;
        speed /= 1_000.0;
    }
    match speed {
        0.0..10.0 => format!("{:.2}{}", speed, SPEED_SUFFIX[order_of_magnitude]),
        10.0..100.0 => format!("{:.1}{}", speed, SPEED_SUFFIX[order_of_magnitude]),
        _ => format!("{}{}", speed as u64, SPEED_SUFFIX[order_of_magnitude]),
    }
}

impl AppState {
    pub(crate) fn insert(
        &self,
        id: Uuid,
        addr: SocketAddr,
    ) -> (mpsc::Sender<Bytes>, StreamingBody) {
        let (tx, rx) = mpsc::channel(128);
        self.conn.insert(
            id,
            SessionData {
                state: SessionState::Start,
                tx: tx.clone(),
            },
        );
        (
            tx,
            StreamingBody {
                rx,
                state: self.clone(),
                addr,
                id,
            },
        )
    }

    pub(crate) fn start_download(&self, id: Uuid) -> Option<mpsc::Sender<Bytes>> {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, tx, .. } = session_data.value_mut()
            && let SessionState::Start = state
        {
            *state = SessionState::Downloading {
                start: Instant::now(),
                average: 0.0,
                total_weights: 0.0,
            };
            Some(tx.clone())
        } else {
            None
        }
    }

    pub(crate) fn measure_download(
        &self,
        id: Uuid,
        instant: Instant,
        size: usize,
    ) -> Option<(mpsc::Sender<Bytes>, String)> {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, tx, .. } = session_data.value_mut()
            && let SessionState::Downloading {
                start,
                average,
                total_weights,
                ..
            } = state
        {
            let speed = calculate_bps(instant, size);
            let weight = calculate_weight(*start, size);
            let new_weights = *total_weights + weight;
            let new_average = (*average * *total_weights + speed * weight) / new_weights;
            *average = new_average;
            *total_weights = new_weights;
            Some((tx.clone(), bps_to_string(*average)))
        } else {
            None
        }
    }

    pub(crate) fn stop_download(&self, id: Uuid) {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, .. } = session_data.value_mut()
            && let SessionState::Downloading { average, .. } = state
        {
            *state = SessionState::End {
                _download: bps_to_string(*average),
            };
        }
    }

    pub(crate) fn finish(&self, id: Uuid) {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, tx, .. } = session_data.value_mut()
            && let SessionState::End { .. } = state
        {
            let _ = tx.try_send(Bytes::new());
        }
    }

    pub(crate) fn remove(&self, id: Uuid) {
        self.conn.remove(&id);
    }
}
