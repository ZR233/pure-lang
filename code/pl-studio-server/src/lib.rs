//! Loopback HTTP/OpenAPI/SSE adapter for [`pl_studio_runtime::StudioRuntime`].

mod error;
mod routes;
mod security;
mod skills_schema;
mod sse;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::middleware;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use pl_studio_runtime::{StudioRuntime, StudioRuntimeOptions};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use utoipa::openapi::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:1421";
pub const MAX_JSON_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_REQUESTS: usize = 64;
const MAX_STREAMS: usize = 64;

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub listen: SocketAddr,
    pub studio_home: Option<PathBuf>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    runtime: StudioRuntime,
    normal_requests: Arc<Semaphore>,
    streams: Arc<Semaphore>,
    shutdown: CancellationToken,
}

impl AppState {
    fn new(runtime: StudioRuntime, shutdown: CancellationToken) -> Self {
        Self {
            runtime,
            normal_requests: Arc::new(Semaphore::new(MAX_REQUESTS)),
            streams: Arc::new(Semaphore::new(MAX_STREAMS)),
            shutdown,
        }
    }
}

pub fn openapi_document() -> OpenApi {
    routes::api_router().into_openapi()
}

pub fn openapi_json() -> anyhow::Result<String> {
    serde_json::to_string_pretty(&openapi_document()).context("failed to serialize OpenAPI")
}

pub async fn serve(options: ServerOptions) -> anyhow::Result<()> {
    security::ensure_loopback_bind(options.listen)?;
    let listener = TcpListener::bind(options.listen)
        .await
        .with_context(|| format!("failed to bind {}", options.listen))?;
    let runtime =
        StudioRuntime::with_options(StudioRuntimeOptions::http_server(options.studio_home))
            .await
            .map_err(anyhow::Error::new)?;
    runtime.start_runtime().await?;
    let shutdown = CancellationToken::new();
    let app = router(AppState::new(runtime.clone(), shutdown.clone()));
    let signal = shutdown_signal(shutdown.clone());

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(signal)
        .await
        .context("Studio HTTP server failed");
    shutdown.cancel();
    let shutdown_result = runtime.shutdown_runtime().await;
    result?;
    shutdown_result?;
    Ok(())
}

fn router(state: AppState) -> Router {
    let (router, openapi) = routes::api_router().split_for_parts();
    router
        .merge(SwaggerUi::new("/docs").url("/openapi.json", openapi))
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            limit_normal_requests,
        ))
        .layer(middleware::from_fn(security::validate_request))
        .with_state(state)
}

async fn limit_normal_requests(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if path == "/api/v1/events/product"
        || (path.starts_with("/api/v1/threads/") && path.ends_with("/events"))
    {
        return next.run(request).await;
    }
    let Ok(_permit) = state.normal_requests.clone().try_acquire_owned() else {
        return error::ApiError::overloaded().into_response();
    };
    next.run(request).await
}

async fn shutdown_signal(shutdown: CancellationToken) {
    wait_for_signal().await;
    shutdown.cancel();
    tokio::spawn(async {
        wait_for_signal().await;
        std::process::exit(130);
    });
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                if let Some(signal) = terminate.as_mut() {
                    signal.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
