use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use pl_protocol::studio::{StudioError, StudioErrorCode};

pub(crate) struct ApiError(pub StudioError);

impl ApiError {
    pub(crate) fn overloaded() -> Self {
        Self(StudioError::new(
            StudioErrorCode::Overloaded,
            "Studio HTTP concurrency limit reached",
            true,
        ))
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self(StudioError::new(
            StudioErrorCode::PermissionDenied,
            message,
            false,
        ))
    }
}

impl From<StudioError> for ApiError {
    fn from(error: StudioError) -> Self {
        Self(error)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(pl_studio_runtime::studio_error_from_anyhow(error))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0.code {
            StudioErrorCode::InvalidArgument | StudioErrorCode::Protocol => StatusCode::BAD_REQUEST,
            StudioErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
            StudioErrorCode::NotFound => StatusCode::NOT_FOUND,
            StudioErrorCode::Busy
            | StudioErrorCode::Conflict
            | StudioErrorCode::StaleRevision
            | StudioErrorCode::Cancelled
            | StudioErrorCode::CancellationTooLate
            | StudioErrorCode::InstanceBusy => StatusCode::CONFLICT,
            StudioErrorCode::Overloaded => StatusCode::TOO_MANY_REQUESTS,
            StudioErrorCode::NotInitialized
            | StudioErrorCode::RuntimeStopped
            | StudioErrorCode::Unavailable
            | StudioErrorCode::Storage
            | StudioErrorCode::Update => StatusCode::SERVICE_UNAVAILABLE,
            StudioErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let correlation_id = self.0.correlation_id.clone();
        let mut response = (status, axum::Json(self.0)).into_response();
        if let Ok(value) = HeaderValue::from_str(&correlation_id) {
            response
                .headers_mut()
                .insert(HeaderName::from_static("x-correlation-id"), value);
        }
        response
    }
}
