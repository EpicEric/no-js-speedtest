use std::{
    net::IpAddr,
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
    addr: IpAddr,
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
        counter: usize,
        bandwidth_average: f64,
        bandwidth_total_weights: f64,
        latency_average: f64,
        latency_total_weights: f64,
    },
    End,
}

#[derive(Clone)]
pub(crate) struct SessionSender(mpsc::Sender<Bytes>);

pub(crate) struct SessionSenderPermit<'a>(mpsc::Permit<'a, Bytes>);

impl SessionSender {
    pub(crate) async fn send(&self, bytes: Bytes) {
        debug_assert!(!bytes.is_empty(), "cannot send empty bytes");
        let _ = self.0.send(bytes).await;
    }

    pub(crate) async fn reserve(&self) -> Option<SessionSenderPermit<'_>> {
        self.0.reserve().await.ok().map(SessionSenderPermit)
    }

    async fn finish(&self) {
        let _ = self.0.send(Bytes::new()).await;
    }
}

impl<'a> SessionSenderPermit<'a> {
    pub(crate) fn send(self, bytes: Bytes) {
        debug_assert!(!bytes.is_empty(), "cannot send empty bytes");
        self.0.send(bytes);
    }
}

pub(crate) struct SessionData {
    state: SessionState,
    sender: SessionSender,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) conn: Arc<DashMap<Uuid, SessionData, RandomState>>,
    pub(crate) max_upload_size: String,
}

impl AppState {
    pub(crate) fn insert(&self, id: Uuid, addr: IpAddr) -> (SessionSender, StreamingBody) {
        let (tx, rx) = mpsc::channel(128);
        let sender = SessionSender(tx);
        self.conn.insert(
            id,
            SessionData {
                state: SessionState::Start,
                sender: sender.clone(),
            },
        );
        (
            sender,
            StreamingBody {
                rx,
                state: self.clone(),
                addr,
                id,
            },
        )
    }

    pub(crate) fn start_download(&self, id: Uuid) -> Option<(SessionSender, Instant)> {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, sender, .. } = session_data.value_mut()
            && let SessionState::Start = state
        {
            let start = Instant::now();
            *state = SessionState::Downloading {
                start,
                counter: 0,
                bandwidth_average: 0.0,
                bandwidth_total_weights: 0.0,
                latency_average: 0.0,
                latency_total_weights: 0.0,
            };
            Some((sender.clone(), start))
        } else {
            None
        }
    }

    pub(crate) fn measure_download_latency(&self, id: Uuid, timestamp: f64, counter: usize) {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, .. } = session_data.value_mut()
            && let SessionState::Downloading {
                start,
                counter: session_counter,
                latency_average: average,
                latency_total_weights: total_weights,
                ..
            } = state
            && counter >= *session_counter
        {
            let latency = (start.elapsed().as_secs_f64() - timestamp) / 2.0;
            let new_weights = *total_weights + 1.0;
            let new_average = (*average * *total_weights + latency) / new_weights;
            *session_counter = counter;
            *average = new_average;
            *total_weights = new_weights;
        }
    }

    pub(crate) fn measure_download_bandwidth(
        &self,
        id: Uuid,
        instant: Instant,
        size: usize,
    ) -> Option<(SessionSender, String, String, Instant)> {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, sender, .. } = session_data.value_mut()
            && let SessionState::Downloading {
                start,
                bandwidth_average: average,
                bandwidth_total_weights: total_weights,
                latency_average,
                ..
            } = state
        {
            let speed = calculate_bps(instant.elapsed(), size);
            let weight = calculate_bandwidth_weight(start.elapsed(), size);
            let new_weights = *total_weights + weight;
            let new_average = (*average * *total_weights + speed * weight) / new_weights;
            *average = new_average;
            *total_weights = new_weights;
            Some((
                sender.clone(),
                bps_to_string(*average),
                seconds_to_string(*latency_average),
                *start,
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

    pub(crate) async fn finish(&self, id: Uuid) {
        if let Some(mut session_data) = self.conn.get_mut(&id)
            && let SessionData { state, sender, .. } = session_data.value_mut()
            && let SessionState::End = state
        {
            sender.finish().await;
        }
    }

    pub(crate) fn remove(&self, id: Uuid) {
        self.conn.remove(&id);
    }
}
