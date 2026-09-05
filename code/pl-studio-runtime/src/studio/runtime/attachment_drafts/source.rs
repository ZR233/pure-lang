//! 附件来源读取：本地文件读取与受约束的 HTTPS 远程快照下载（重定向、公网地址与字节上限）。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use reqwest::header::LOCATION;
use tokio::io::AsyncReadExt;
use url::Url;

pub(super) const MAX_IMAGE_SOURCE_BYTES: u64 = 20 * 1024 * 1024;
pub(super) const MAX_GENERIC_SOURCE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const REMOTE_FETCH_TOTAL_TIMEOUT: Duration = Duration::from_secs(45);

pub(super) async fn read_local_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let file = tokio::fs::File::open(path)
        .await
        .context("failed to open attachment file")?;
    let metadata = file
        .metadata()
        .await
        .context("failed to inspect attachment file")?;
    ensure!(metadata.is_file(), "attachment source is not a file");
    ensure!(metadata.len() <= max_bytes, "attachment file is too large");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .context("failed to read attachment file")?;
    ensure!(
        bytes.len() as u64 <= max_bytes,
        "attachment file is too large"
    );
    Ok(bytes)
}

pub(super) struct LoadedSource {
    pub(super) bytes: Vec<u8>,
    pub(super) initial_remote_url: Option<String>,
}

pub(super) async fn fetch_remote_snapshot(raw_url: &str, max_bytes: u64) -> Result<LoadedSource> {
    tokio::time::timeout(
        REMOTE_FETCH_TOTAL_TIMEOUT,
        fetch_remote_snapshot_with_redirects(raw_url, max_bytes),
    )
    .await
    .context("attachment URL fetch exceeded the total timeout")?
}

async fn fetch_remote_snapshot_with_redirects(
    raw_url: &str,
    max_bytes: u64,
) -> Result<LoadedSource> {
    let mut url = validate_remote_url(Url::parse(raw_url).context("invalid attachment URL")?)?;
    let original = url.as_str().to_string();
    for redirect in 0..=MAX_REDIRECTS {
        let host = url
            .host_str()
            .context("attachment URL has no host")?
            .to_string();
        let port = url
            .port_or_known_default()
            .context("attachment URL has no port")?;
        let addresses = tokio::net::lookup_host((host.as_str(), port))
            .await
            .context("attachment URL host cannot be resolved")?
            .collect::<Vec<_>>();
        ensure!(
            !addresses.is_empty(),
            "attachment URL host has no addresses"
        );
        for address in &addresses {
            ensure_public_ip(address.ip())?;
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .resolve(&host, addresses[0])
            .build()?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .context("attachment URL fetch failed")?;
        if response.status().is_redirection() {
            ensure!(
                redirect < MAX_REDIRECTS,
                "attachment URL has too many redirects"
            );
            let location = response
                .headers()
                .get(LOCATION)
                .context("attachment redirect has no Location")?
                .to_str()
                .context("attachment redirect Location is invalid")?;
            url = validate_remote_url(url.join(location).context("invalid attachment redirect")?)?;
            continue;
        }
        ensure!(
            response.status().is_success(),
            "attachment URL returned an error status"
        );
        if let Some(length) = response.content_length() {
            ensure!(length <= max_bytes, "remote attachment is too large");
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("remote attachment stream failed")?;
            ensure!(
                bytes.len().saturating_add(chunk.len()) as u64 <= max_bytes,
                "remote attachment is too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        return Ok(LoadedSource {
            bytes,
            initial_remote_url: Some(original),
        });
    }
    bail!("attachment URL redirect handling failed")
}

pub(super) fn validate_remote_url(url: Url) -> Result<Url> {
    ensure!(url.scheme() == "https", "attachment URL must use HTTPS");
    ensure!(
        url.username().is_empty(),
        "attachment URL must not contain credentials"
    );
    ensure!(
        url.password().is_none(),
        "attachment URL must not contain credentials"
    );
    ensure!(
        url.fragment().is_none(),
        "attachment URL must not contain a fragment"
    );
    let host = url.host_str().context("attachment URL has no host")?;
    ensure!(
        !host.eq_ignore_ascii_case("localhost"),
        "attachment URL host is not public"
    );
    if let Ok(ip) = host.parse::<IpAddr>() {
        ensure_public_ip(ip)?;
    }
    Ok(url)
}

fn ensure_public_ip(ip: IpAddr) -> Result<()> {
    let public = match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    };
    ensure!(public, "attachment URL resolved to a non-public address");
    Ok(())
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(ipv4);
    }
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x2001 && segments[1] == 0x0002)
        || (segments[0] == 0x0100 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0)
        || (segments[0] & 0xffc0) == 0xfec0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_metadata_addresses_are_rejected() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "192.168.1.2",
            "100.64.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
            "2001:2::1",
            "::ffff:127.0.0.1",
            "fec0::1",
        ] {
            assert!(ensure_public_ip(ip.parse().unwrap()).is_err(), "{ip}");
        }
    }

    #[test]
    fn url_credentials_fragments_and_non_https_are_rejected() {
        for url in [
            "http://example.com/a.png",
            "https://user:secret@example.com/a.png",
            "https://example.com/a.png#fragment",
            "https://localhost/a.png",
        ] {
            assert!(
                validate_remote_url(Url::parse(url).unwrap()).is_err(),
                "{url}"
            );
        }
    }
}
