use std::{
    pin::Pin,
    sync::OnceLock,
    task::{Context, Poll},
    time::Instant,
};

use askama::Template;
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame};
use uuid::Uuid;

use crate::{session::AppState, templates::DownloadTemplate};

pub(crate) static RANDOM_BITMAP: OnceLock<Bytes> = OnceLock::new();

pub(crate) struct DownloadBody {
    pub(crate) instant: Instant,
    pub(crate) state: AppState,
    pub(crate) id: Uuid,
    pub(crate) size: usize,
    pub(crate) counter: usize,
    pub(crate) is_end_stream: bool,
}

pub(crate) static DOWNLOAD_TEST_DURATION: u64 = 15;

impl HttpBody for DownloadBody {
    type Data = Bytes;

    type Error = color_eyre::Report;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.is_end_stream {
            Poll::Ready(None)
        } else {
            self.is_end_stream = true;
            Poll::Ready(Some(Ok(Frame::data(
                RANDOM_BITMAP.get().unwrap().slice(0..self.size),
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
        if let Some((tx, download_speed, download_latency, timestamp)) = self
            .state
            .measure_download_bandwidth(self.id, self.instant, self.size)
        {
            let html = DownloadTemplate {
                id: self.id,
                next_size: match self.counter {
                    0 => 10_000_000,
                    1 => 25_000_000,
                    2 => 50_000_000,
                    3 => 75_000_000,
                    4.. => 100_000_000,
                },
                counter: self.counter + 1,
                download_speed,
                download_latency,
                timestamp,
            };
            let _ = tx.try_send(Bytes::from(html.render().unwrap()));
        }
    }
}
