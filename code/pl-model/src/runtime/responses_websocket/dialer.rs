use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use tokio::net::TcpStream;
use tokio::time::{Instant, sleep_until};
use tokio_tungstenite::tungstenite::Error as WebSocketError;
use tokio_tungstenite::tungstenite::handshake::client::{Request, Response};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, client_async_tls_with_config};

const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(250);

pub(super) async fn connect(
    request: Request,
    url: &reqwest::Url,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), WebSocketError> {
    let addresses = resolve_addresses(url).await.map_err(WebSocketError::Io)?;
    let stream = connect_happy_eyeballs(addresses, TcpStream::connect)
        .await
        .map_err(WebSocketError::Io)?;
    if let Ok(remote_address) = stream.peer_addr() {
        tracing::trace!(
            address_family = address_family(remote_address),
            %remote_address,
            "Responses WebSocket TCP connection selected"
        );
    }
    client_async_tls_with_config(request, stream, None, None).await
}

async fn resolve_addresses(url: &reqwest::Url) -> io::Result<Vec<SocketAddr>> {
    let host = url
        .host_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WebSocket URL has no host"))?;
    let port = url.port_or_known_default().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebSocket URL has no known port",
        )
    })?;
    tokio::net::lookup_host((host, port))
        .await
        .map(|addresses| addresses.collect())
}

async fn connect_happy_eyeballs<T, F, Fut>(
    addresses: Vec<SocketAddr>,
    mut connect: F,
) -> io::Result<T>
where
    F: FnMut(SocketAddr) -> Fut,
    Fut: Future<Output = io::Result<T>>,
{
    let mut addresses = addresses.into_iter();
    let Some(first_address) = addresses.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebSocket host did not resolve to any addresses",
        ));
    };

    let first_is_ipv4 = first_address.is_ipv4();
    let mut preferred = VecDeque::new();
    let mut alternate = VecDeque::new();
    for address in addresses {
        if address.is_ipv4() == first_is_ipv4 {
            preferred.push_back(address);
        } else {
            alternate.push_back(address);
        }
    }

    let mut addresses = VecDeque::new();
    while !preferred.is_empty() || !alternate.is_empty() {
        if let Some(address) = alternate.pop_front() {
            addresses.push_back(address);
        }
        if let Some(address) = preferred.pop_front() {
            addresses.push_back(address);
        }
    }

    let mut attempts = FuturesUnordered::new();
    tracing::trace!(
        address_family = address_family(first_address),
        remote_address = %first_address,
        "attempting Responses WebSocket TCP connection"
    );
    attempts.push(connect_address(first_address, connect(first_address)));
    let mut next_attempt_at = Instant::now() + HAPPY_EYEBALLS_DELAY;
    let mut last_error = None;

    loop {
        if addresses.is_empty() {
            match attempts.next().await {
                Some((_, Ok(stream))) => return Ok(stream),
                Some((address, Err(error))) => {
                    tracing::trace!(
                        address_family = address_family(address),
                        remote_address = %address,
                        error_kind = ?error.kind(),
                        "Responses WebSocket TCP connection failed"
                    );
                    if attempts.is_empty() {
                        return Err(error);
                    }
                    last_error = Some(error);
                }
                None => {
                    return Err(last_error.unwrap_or_else(|| {
                        io::Error::other("WebSocket connection attempts ended without an error")
                    }));
                }
            }
            continue;
        }

        tokio::select! {
            result = attempts.next() => {
                match result {
                    Some((_, Ok(stream))) => return Ok(stream),
                    Some((address, Err(error))) => {
                        tracing::trace!(
                            address_family = address_family(address),
                            remote_address = %address,
                            error_kind = ?error.kind(),
                            "Responses WebSocket TCP connection failed"
                        );
                        last_error = Some(error);
                        let address = take_next_address(&mut addresses)?;
                        tracing::trace!(
                            address_family = address_family(address),
                            remote_address = %address,
                            "attempting Responses WebSocket TCP connection"
                        );
                        attempts.push(connect_address(address, connect(address)));
                        next_attempt_at = Instant::now() + HAPPY_EYEBALLS_DELAY;
                    }
                    None => {
                        let address = take_next_address(&mut addresses)?;
                        tracing::trace!(
                            address_family = address_family(address),
                            remote_address = %address,
                            "attempting Responses WebSocket TCP connection"
                        );
                        attempts.push(connect_address(address, connect(address)));
                        next_attempt_at = Instant::now() + HAPPY_EYEBALLS_DELAY;
                    }
                }
            }
            _ = sleep_until(next_attempt_at) => {
                let address = take_next_address(&mut addresses)?;
                tracing::trace!(
                    address_family = address_family(address),
                    remote_address = %address,
                    "attempting alternate Responses WebSocket TCP connection"
                );
                attempts.push(connect_address(address, connect(address)));
                next_attempt_at = Instant::now() + HAPPY_EYEBALLS_DELAY;
            }
        }
    }
}

async fn connect_address<T, Fut>(address: SocketAddr, connect: Fut) -> (SocketAddr, io::Result<T>)
where
    Fut: Future<Output = io::Result<T>>,
{
    (address, connect.await)
}

fn take_next_address(addresses: &mut VecDeque<SocketAddr>) -> io::Result<SocketAddr> {
    addresses
        .pop_front()
        .ok_or_else(|| io::Error::other("WebSocket address queue unexpectedly empty"))
}

fn address_family(address: SocketAddr) -> &'static str {
    if address.is_ipv4() { "ipv4" } else { "ipv6" }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use pretty_assertions::assert_eq;
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn uses_alternate_family_without_waiting_for_stalled_preferred() {
        let stalled = "[2001:db8::1]:443"
            .parse::<SocketAddr>()
            .expect("stalled address should parse");
        let reachable = "127.0.0.1:443"
            .parse::<SocketAddr>()
            .expect("reachable address should parse");

        let connected = timeout(
            Duration::from_secs(1),
            connect_happy_eyeballs(vec![stalled, reachable], |address| async move {
                if address == stalled {
                    std::future::pending::<()>().await;
                }
                Ok(address)
            }),
        )
        .await
        .expect("alternate family should start before timeout")
        .expect("alternate family should connect");

        assert_eq!(connected, reachable);
    }

    #[tokio::test]
    async fn fast_preferred_address_does_not_start_an_alternate_attempt() {
        let preferred = "[2001:db8::1]:443"
            .parse::<SocketAddr>()
            .expect("preferred address should parse");
        let alternate = "127.0.0.1:443"
            .parse::<SocketAddr>()
            .expect("alternate address should parse");
        let attempt_count = Arc::new(AtomicUsize::new(0));

        let connected = connect_happy_eyeballs(vec![preferred, alternate], {
            let attempt_count = Arc::clone(&attempt_count);
            move |address| {
                let attempt_count = Arc::clone(&attempt_count);
                async move {
                    attempt_count.fetch_add(1, Ordering::Relaxed);
                    Ok(address)
                }
            }
        })
        .await
        .expect("preferred address should connect");

        assert_eq!(connected, preferred);
        assert_eq!(attempt_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn returns_the_last_error_when_all_addresses_fail() {
        let first = "[2001:db8::1]:443"
            .parse::<SocketAddr>()
            .expect("first address should parse");
        let last = "127.0.0.1:443"
            .parse::<SocketAddr>()
            .expect("last address should parse");

        let error = connect_happy_eyeballs(vec![first, last], |address| async move {
            Err::<SocketAddr, _>(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                address.to_string(),
            ))
        })
        .await
        .expect_err("all failed addresses should return an error");

        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
        assert_eq!(error.to_string(), last.to_string());
    }

    #[tokio::test]
    async fn rejects_an_empty_address_list() {
        let error = connect_happy_eyeballs(Vec::new(), |address| async move { Ok(address) })
            .await
            .expect_err("empty address list should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "WebSocket host did not resolve to any addresses"
        );
    }
}
