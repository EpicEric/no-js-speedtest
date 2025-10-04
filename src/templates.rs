use askama::Template;
use uuid::Uuid;

#[derive(Template)]
#[template(path = "index.html")]
pub(crate) struct IndexTemplate {
    pub(crate) id: Uuid,
}

#[derive(Template)]
#[template(path = "start.html")]
pub(crate) struct StartTemplate {
    pub(crate) id: Uuid,
    pub(crate) test_duration: u64,
}

#[derive(Template)]
#[template(path = "download.html")]
pub(crate) struct DownloadTemplate {
    pub(crate) id: Uuid,
    pub(crate) next_size: usize,
    pub(crate) counter: usize,
    pub(crate) download_speed: String,
}

#[derive(Template)]
#[template(path = "finish_download.html")]
pub(crate) struct FinishDownloadTemplate;
