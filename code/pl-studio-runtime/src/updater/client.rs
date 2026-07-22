use super::error::{StudioUpdateError, StudioUpdateErrorCode};
use super::install;
use super::manifest::{LATEST_MANIFEST_URL, evaluate_manifest, validate_redirect_url};
use super::types::{StudioUpdate, StudioUpdateCheck, StudioUpdateEvent};
use futures::StreamExt;
use reqwest::{Client, Response, StatusCode};
use std::env;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use url::Url;

const MAX_REDIRECTS: usize = 5;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// GitHub Release 稳定更新客户端。
#[derive(Clone)]
pub struct StudioUpdater {
    pub(super) client: Client,
    pub(super) cache_dir: PathBuf,
    pub(super) public_key: &'static str,
    pub(super) install_active: Arc<std::sync::atomic::AtomicBool>,
}

impl StudioUpdater {
    /// 创建使用生产清单、公钥和应用缓存目录的更新客户端。
    pub fn new_default() -> Result<Self, StudioUpdateError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Pure-Studio/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(network_error)?;
        let cache_root = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("Pure Studio")
            .join("updates");
        Ok(Self {
            client,
            cache_dir: cache_root,
            public_key: include_str!("pure-studio.pub"),
            install_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    /// 检查稳定更新；同版或降级统一返回 [`StudioUpdateCheck::UpToDate`]。
    pub async fn check(
        &self,
        current_version: &str,
    ) -> Result<StudioUpdateCheck, StudioUpdateError> {
        let url = Url::parse(LATEST_MANIFEST_URL).map_err(|error| {
            StudioUpdateError::new(
                StudioUpdateErrorCode::InvalidManifest,
                format!("invalid built-in update URL: {error}"),
            )
        })?;
        let bytes = self.request_bytes(url, MAX_MANIFEST_BYTES).await?;
        evaluate_manifest(&bytes, current_version)
    }

    /// 下载并验证更新，在启动安装器前执行调用方提供的最终 runtime 关停检查。
    pub async fn install_after<F, Fut>(
        &self,
        update: StudioUpdate,
        progress: UnboundedSender<StudioUpdateEvent>,
        before_launch: F,
    ) -> Result<(), StudioUpdateError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), StudioUpdateError>>,
    {
        install::install_after(self, update, progress, before_launch).await
    }

    /// 下载、验证并启动安装器。Bridge 应优先使用 [`Self::install_after`] 提供 busy guard。
    pub async fn install(
        &self,
        update: StudioUpdate,
        progress: UnboundedSender<StudioUpdateEvent>,
    ) -> Result<(), StudioUpdateError> {
        self.install_after(update, progress, || async { Ok(()) })
            .await
    }

    pub(super) async fn request(&self, mut url: Url) -> Result<Response, StudioUpdateError> {
        for redirect_count in 0..=MAX_REDIRECTS {
            validate_redirect_url(&url)?;
            let response = self
                .client
                .get(url.clone())
                .send()
                .await
                .map_err(network_error)?;
            if response.status().is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(StudioUpdateError::new(
                        StudioUpdateErrorCode::Network,
                        "update download exceeded five redirects",
                    ));
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .ok_or_else(|| {
                        StudioUpdateError::new(
                            StudioUpdateErrorCode::Network,
                            "update redirect is missing Location",
                        )
                    })?
                    .to_str()
                    .map_err(|_| {
                        StudioUpdateError::new(
                            StudioUpdateErrorCode::Network,
                            "update redirect Location is not UTF-8",
                        )
                    })?;
                url = url.join(location).map_err(|error| {
                    StudioUpdateError::new(
                        StudioUpdateErrorCode::Network,
                        format!("invalid update redirect: {error}"),
                    )
                })?;
                continue;
            }
            if response.status() != StatusCode::OK {
                return Err(StudioUpdateError::new(
                    StudioUpdateErrorCode::Network,
                    format!("update request failed with HTTP {}", response.status()),
                ));
            }
            return Ok(response);
        }
        Err(StudioUpdateError::new(
            StudioUpdateErrorCode::Network,
            "update redirect loop did not terminate",
        ))
    }

    pub(super) async fn request_bytes(
        &self,
        url: Url,
        max_bytes: u64,
    ) -> Result<Vec<u8>, StudioUpdateError> {
        let response = self.request(url).await?;
        if response
            .content_length()
            .is_some_and(|size| size > max_bytes)
        {
            return Err(StudioUpdateError::new(
                StudioUpdateErrorCode::DownloadTooLarge,
                "update response exceeds the allowed size",
            ));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(network_error)?;
            let next_len = bytes.len().checked_add(chunk.len()).ok_or_else(|| {
                StudioUpdateError::new(
                    StudioUpdateErrorCode::DownloadTooLarge,
                    "update response size overflow",
                )
            })?;
            if u64::try_from(next_len).unwrap_or(u64::MAX) > max_bytes {
                return Err(StudioUpdateError::new(
                    StudioUpdateErrorCode::DownloadTooLarge,
                    "update response exceeds the allowed size",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

fn network_error(error: reqwest::Error) -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::Network,
        format!("update network request failed: {error}"),
    )
}
