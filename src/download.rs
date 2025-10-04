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
            let id = self.id;
            let instant = self.instant;
            let size = self.size;
            let state = self.state.clone();
            let counter = self.counter;
            tokio::spawn(async move {
                if let Some((sender, download_speed, download_latency, instant)) =
                    state.measure_download_bandwidth(id, instant, size)
                {
                    let next_size = match counter {
                        0 => 20_000_000,
                        1 => 30_000_000,
                        2 => 40_000_000,
                        3 => 50_000_000,
                        4 => 60_000_000,
                        5 => 70_000_000,
                        6 => 80_000_000,
                        7 => 90_000_000,
                        8.. => 100_000_000,
                    };
                    if let Some(permit) = sender.reserve().await {
                        let html = DownloadTemplate {
                            id,
                            next_size,
                            counter: counter + 1,
                            download_speed,
                            download_latency,
                            timestamp: instant.elapsed().as_secs_f64(),
                        };
                        permit.send(Bytes::from(html.render().unwrap()));
                    }
                }
            });
            Poll::Ready(None)
        } else {
            self.is_end_stream = true;
            Poll::Ready(Some(Ok(Frame::data(
                RANDOM_BITMAP.get().unwrap().slice(0..self.size),
            ))))
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        if self.is_end_stream {
            http_body::SizeHint::default()
        } else {
            let mut size_hint = http_body::SizeHint::new();
            size_hint.set_lower(self.size as u64);
            size_hint
        }
    }
}
