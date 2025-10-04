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

use crate::utils::{bps_to_string, calculate_bandwidth_weight, calculate_bps, seconds_to_string};

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
        bandwidth_average: f64,
        bandwidth_total_weights: f64,
        latency_average: f64,
        latency_total_weights: f64,
    },
    End,
}

pub(crate) struct SessionData {
    state: SessionState,
    tx: mpsc::Sender<Bytes>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) conn: Arc<DashMap<Uuid, SessionData, RandomState>>,
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
                bandwidth_average: 0.0,
                bandwidth_total_weights: 0.0,
                latency_average: 0.0,
                latency_total_weights: 0.0,
            };
            Some(tx.clone())
        } else {
            None
        }
    }

    pub(crate) fn measure_download_latency(&self, id: Uuid, timestamp: f64) {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, .. } = session_data.value_mut()
            && let SessionState::Downloading {
                start,
                latency_average: average,
                latency_total_weights: total_weights,
                ..
            } = state
        {
            let latency = start.elapsed().as_secs_f64() - timestamp;
            println!("latency: {latency}");
            let new_weights = *total_weights + 1.0;
            let new_average = (*average * *total_weights + latency) / new_weights;
            *average = new_average;
            *total_weights = new_weights;
        }
    }

    pub(crate) fn measure_download_bandwidth(
        &self,
        id: Uuid,
        instant: Instant,
        size: usize,
    ) -> Option<(mpsc::Sender<Bytes>, String, String, f64)> {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, tx, .. } = session_data.value_mut()
            && let SessionState::Downloading {
                start,
                bandwidth_average: average,
                bandwidth_total_weights: total_weights,
                latency_average,
                ..
            } = state
        {
            let speed = calculate_bps(instant, size);
            let weight = calculate_bandwidth_weight(*start, size);
            let new_weights = *total_weights + weight;
            let new_average = (*average * *total_weights + speed * weight) / new_weights;
            *average = new_average;
            *total_weights = new_weights;
            Some((
                tx.clone(),
                bps_to_string(*average),
                seconds_to_string(*latency_average),
                start.elapsed().as_secs_f64(),
            ))
        } else {
            None
        }
    }

    pub(crate) fn stop_download(&self, id: Uuid) -> Option<(String, String)> {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, .. } = session_data.value_mut()
            && let SessionState::Downloading {
                bandwidth_average,
                latency_average,
                ..
            } = state
        {
            let download_bandwidth = bps_to_string(*bandwidth_average);
            let download_latency = seconds_to_string(*latency_average);
            *state = SessionState::End;
            Some((download_bandwidth, download_latency))
        } else {
            None
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
