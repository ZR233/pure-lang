//! Loopback HTTP/OpenAPI/SSE adapter for [`pl_studio_runtime::StudioRuntime`].

mod error;
mod routes;
mod security;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use pl_protocol::studio::StudioOperation;
    use tower::ServiceExt;

    async fn test_runtime() -> StudioRuntime {
        let config_home = tempfile::tempdir().unwrap().keep();
        StudioRuntime::with_options(StudioRuntimeOptions {
            studio_home: Some(config_home),
            host: pl_studio_runtime::StudioHostKind::Test,
        })
        .await
        .unwrap()
    }

    async fn test_app() -> Router {
        router(AppState::new(
            test_runtime().await,
            CancellationToken::new(),
        ))
    }

    fn request(method: &str, uri: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:1421")
            .body(body)
            .unwrap()
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), MAX_JSON_BODY_BYTES + 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn generated_openapi_is_version_31_and_covers_every_shared_operation() {
        let document = openapi_document();
        let value = serde_json::to_value(&document).unwrap();
        assert_eq!(value["openapi"], "3.1.0");
        let reparsed: OpenApi = serde_json::from_value(value.clone()).unwrap_or_else(|error| {
            for (path, item) in value["paths"].as_object().unwrap() {
                let candidate = serde_json::json!({
                    "openapi": value["openapi"].clone(),
                    "info": value["info"].clone(),
                    "paths": { (path): item },
                });
                if let Err(path_error) = serde_json::from_value::<OpenApi>(candidate) {
                    panic!("generated OpenAPI path {path} does not parse: {path_error}");
                }
            }
            for (name, schema) in value["components"]["schemas"].as_object().unwrap() {
                let candidate = serde_json::json!({
                    "openapi": value["openapi"].clone(),
                    "info": value["info"].clone(),
                    "paths": {},
                    "components": { "schemas": { (name): schema } },
                });
                if let Err(schema_error) = serde_json::from_value::<OpenApi>(candidate) {
                    panic!("generated OpenAPI schema {name} does not parse: {schema_error}");
                }
            }
            panic!("generated OpenAPI does not parse: {error}");
        });
        assert_eq!(
            serde_json::to_value(reparsed).unwrap()["openapi"],
            serde_json::to_value(document).unwrap()["openapi"]
        );

        let mut ids = std::collections::BTreeSet::new();
        for item in value["paths"].as_object().unwrap().values() {
            for operation in item.as_object().unwrap().values() {
                if let Some(id) = operation.get("operationId").and_then(|id| id.as_str()) {
                    assert!(ids.insert(id.to_string()), "duplicate operationId {id}");
                }
            }
        }
        assert!(ids.remove("health"));
        let expected = StudioOperation::ALL
            .into_iter()
            .map(|operation| operation.operation_id().to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids, expected);
    }

    #[test]
    fn generated_openapi_has_no_dangling_schema_references() {
        let value = serde_json::to_value(openapi_document()).unwrap();
        let schemas = value["components"]["schemas"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mut references = std::collections::BTreeSet::new();
        collect_schema_references(&value, &mut references);

        let dangling = references
            .difference(&schemas)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert!(dangling.is_empty(), "dangling refs: {dangling:?}");
    }

    #[tokio::test]
    async fn health_openapi_and_swagger_are_served_on_fixed_paths() {
        let app = test_app().await;
        let health = app
            .clone()
            .oneshot(request("GET", "/health", Body::empty()))
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(json_body(health).await["status"], "ok");

        let openapi = app
            .clone()
            .oneshot(request("GET", "/openapi.json", Body::empty()))
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        assert_eq!(json_body(openapi).await["openapi"], "3.1.0");

        let docs = app
            .oneshot(request("GET", "/docs", Body::empty()))
            .await
            .unwrap();
        assert!(docs.status().is_success() || docs.status().is_redirection());
    }

    #[tokio::test]
    async fn malformed_and_oversized_json_use_the_typed_error_envelope() {
        let app = test_app().await;
        let unknown_field = Request::builder()
            .method("POST")
            .uri("/api/v1/projects")
            .header("host", "localhost:1421")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"path":"/tmp/project","unknown":true}"#))
            .unwrap();
        let response = app.clone().oneshot(unknown_field).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let correlation = response
            .headers()
            .get("x-correlation-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let error = json_body(response).await;
        assert_eq!(error["code"], "invalidArgument");
        assert_eq!(error["correlationId"], correlation);

        let oversized = Request::builder()
            .method("POST")
            .uri("/api/v1/projects")
            .header("host", "localhost:1421")
            .header("content-type", "application/json")
            .body(Body::from("x".repeat(MAX_JSON_BODY_BYTES + 1)))
            .unwrap();
        let response = app.oneshot(oversized).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["code"], "invalidArgument");
    }

    #[tokio::test]
    async fn settings_cas_returns_stale_revision_as_http_conflict() {
        let app = test_app().await;
        let settings = app
            .clone()
            .oneshot(request("GET", "/api/v1/settings", Body::empty()))
            .await
            .unwrap();
        let revision = json_body(settings).await["revision"].as_u64().unwrap();
        let body = serde_json::json!({
            "expectedRevision": revision,
            "mode": "auto-review",
        })
        .to_string();
        let save = Request::builder()
            .method("PUT")
            .uri("/api/v1/settings/permission")
            .header("host", "localhost:1421")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(save).await.unwrap().status(),
            StatusCode::OK
        );

        let stale = Request::builder()
            .method("PUT")
            .uri("/api/v1/settings/permission")
            .header("host", "localhost:1421")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let response = app.oneshot(stale).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(json_body(response).await["code"], "staleRevision");
    }

    #[tokio::test]
    async fn invalid_typed_settings_return_bad_request_instead_of_internal_error() {
        let app = test_app().await;
        let settings = app
            .clone()
            .oneshot(request("GET", "/api/v1/settings", Body::empty()))
            .await
            .unwrap();
        let revision = json_body(settings).await["revision"].as_u64().unwrap();
        let body = serde_json::json!({
            "expectedRevision": revision,
            "mode": "future-mode",
            "allowedDomains": [],
        })
        .to_string();
        let save = Request::builder()
            .method("PUT")
            .uri("/api/v1/settings/web-search")
            .header("host", "localhost:1421")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(save).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["code"], "invalidArgument");
    }

    #[tokio::test]
    async fn thread_sse_starts_with_authoritative_snapshot_and_stale_on_reconnect() {
        use futures::StreamExt as _;

        let runtime = test_runtime().await;
        runtime.start_runtime().await.unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let git = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(workspace.path())
            .status()
            .unwrap();
        assert!(git.success());
        let project = runtime.open_project(workspace.path()).await.unwrap();
        let thread = runtime
            .create_thread(&project.id, "SSE contract")
            .await
            .unwrap();
        let app = router(AppState::new(runtime.clone(), CancellationToken::new()));

        let response = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/api/v1/threads/{}/events", thread.id),
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&first).contains("event: snapshot"));
        drop(stream);

        let reconnect = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/threads/{}/events", thread.id))
            .header("host", "localhost:1421")
            .header("last-event-id", "thread:1")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(reconnect).await.unwrap();
        let mut stream = response.into_body().into_data_stream();
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&first).contains("event: stale"));
        drop(stream);
        runtime.shutdown_runtime().await.unwrap();
    }

    #[tokio::test]
    async fn thread_read_returns_the_same_authoritative_snapshot_shape_as_sse() {
        let runtime = test_runtime().await;
        runtime.start_runtime().await.unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let git = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(workspace.path())
            .status()
            .unwrap();
        assert!(git.success());
        let project = runtime.open_project(workspace.path()).await.unwrap();
        let thread = runtime
            .create_thread(&project.id, "Read contract")
            .await
            .unwrap();
        let app = router(AppState::new(runtime.clone(), CancellationToken::new()));

        let response = app
            .oneshot(request(
                "GET",
                &format!("/api/v1/threads/{}", thread.id),
                Body::empty(),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["schemaVersion"], pl_protocol::THREAD_SCHEMA_VERSION);
        assert_eq!(body["thread"]["id"], thread.id);
        assert_eq!(body["items"], serde_json::json!([]));
        assert!(body.get("revision").is_some());
        runtime.shutdown_runtime().await.unwrap();
    }

    fn collect_schema_references(
        value: &serde_json::Value,
        references: &mut std::collections::BTreeSet<String>,
    ) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(|value| value.as_str())
                    && let Some(schema) = reference.strip_prefix("#/components/schemas/")
                {
                    references.insert(schema.to_string());
                }
                for value in object.values() {
                    collect_schema_references(value, references);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    collect_schema_references(value, references);
                }
            }
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_) => {}
        }
    }
}
