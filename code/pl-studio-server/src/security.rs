use std::net::{IpAddr, SocketAddr};

use anyhow::ensure;
use axum::extract::Request;
use axum::http::header::{HOST, ORIGIN};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;

pub(crate) fn ensure_loopback_bind(address: SocketAddr) -> anyhow::Result<()> {
    ensure!(
        address.ip().is_loopback(),
        "pl-studio-server only accepts loopback listen addresses"
    );
    Ok(())
}

pub(crate) async fn validate_request(request: Request, next: Next) -> Response {
    match validate_headers(&request) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

fn validate_headers(request: &Request) -> Result<(), ApiError> {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("A loopback Host header is required"))?;
    let authority = host
        .parse::<axum::http::uri::Authority>()
        .map_err(|_| ApiError::forbidden("Invalid Host header"))?;
    if !is_loopback_host(authority.host()) {
        return Err(ApiError::forbidden("Host must be loopback or localhost"));
    }
    let Some(origin) = request.headers().get(ORIGIN) else {
        return Ok(());
    };
    let origin = origin
        .to_str()
        .ok()
        .and_then(|origin| url::Url::parse(origin).ok())
        .ok_or_else(|| ApiError::forbidden("Invalid Origin header"))?;
    if origin.scheme() != "http"
        || !origin.host_str().is_some_and(|origin_host| {
            origin_host.eq_ignore_ascii_case(normalized_host(authority.host()))
        })
        || origin.port_or_known_default() != Some(authority.port_u16().unwrap_or(80))
    {
        return Err(ApiError::forbidden("Origin must be same-origin with Host"));
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    let host = normalized_host(host);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn normalized_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;

    #[test]
    fn non_loopback_bind_is_rejected() {
        let address = "0.0.0.0:1421".parse().unwrap();
        assert!(ensure_loopback_bind(address).is_err());
    }

    #[test]
    fn loopback_names_and_addresses_are_accepted() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(!is_loopback_host("example.com"));
    }

    #[test]
    fn host_is_required_and_must_be_loopback() {
        let missing = Request::builder().body(Body::empty()).unwrap();
        assert!(validate_headers(&missing).is_err());

        let remote = Request::builder()
            .header(HOST, "example.com:1421")
            .body(Body::empty())
            .unwrap();
        assert!(validate_headers(&remote).is_err());

        let ipv6 = Request::builder()
            .header(HOST, "[::1]:1421")
            .body(Body::empty())
            .unwrap();
        assert!(validate_headers(&ipv6).is_ok());
    }

    #[test]
    fn origin_must_match_loopback_host_and_port() {
        let same_origin = Request::builder()
            .header(HOST, "localhost:1421")
            .header(ORIGIN, "http://localhost:1421")
            .body(Body::empty())
            .unwrap();
        assert!(validate_headers(&same_origin).is_ok());

        let wrong_port = Request::builder()
            .header(HOST, "localhost:1421")
            .header(ORIGIN, "http://localhost:1422")
            .body(Body::empty())
            .unwrap();
        assert!(validate_headers(&wrong_port).is_err());

        let https = Request::builder()
            .header(HOST, "127.0.0.1:1421")
            .header(ORIGIN, "https://127.0.0.1:1421")
            .body(Body::empty())
            .unwrap();
        assert!(validate_headers(&https).is_err());
    }
}
