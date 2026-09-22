//! Attempt-local CONNECT proxy for an image's fixed egress-forwarder contract.
//!
//! The container is still run with `--network=none`: the Unix socket here is
//! mounted into it and the in-image forwarder is responsible for exposing a
//! loopback-only HTTP proxy.  This server accepts CONNECT only and pins the
//! already validated DNS result for the lifetime of the upstream connection.

use anyhow::{anyhow, bail, ensure, Context, Result};
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, TcpStream, UnixListener, UnixStream};
use tokio::sync::{watch, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{timeout, Duration};

use super::profile::validate_tls_domain;

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressPolicy {
    allowed_hosts: BTreeSet<String>,
    max_connections: usize,
}

impl EgressPolicy {
    pub fn new(domains: impl IntoIterator<Item = String>, max_connections: usize) -> Result<Self> {
        ensure!(
            (1..=64).contains(&max_connections),
            "egress max_connections must be between 1 and 64"
        );
        let mut allowed_hosts = BTreeSet::new();
        for domain in domains {
            allowed_hosts.insert(validate_tls_domain(&domain)?);
        }
        ensure!(
            !allowed_hosts.is_empty(),
            "egress allowlist cannot be empty"
        );
        Ok(Self {
            allowed_hosts,
            max_connections,
        })
    }

    pub fn allows(&self, host: &str) -> bool {
        validate_tls_domain(host).is_ok_and(|host| self.allowed_hosts.contains(&host))
    }
}

pub struct EgressProxy {
    socket_path: PathBuf,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
    successful_connects: Arc<AtomicU64>,
}

impl EgressProxy {
    /// Start a proxy at a fresh, attempt-owned socket path. Callers must create
    /// a unique temporary directory; this method refuses to replace a socket.
    pub async fn start(socket_path: PathBuf, policy: EgressPolicy) -> Result<Self> {
        ensure!(
            socket_path.is_absolute(),
            "egress socket path must be absolute"
        );
        ensure!(
            !socket_path.exists(),
            "refusing to replace an existing egress socket"
        );
        let listener = UnixListener::bind(&socket_path).context("bind attempt egress socket")?;
        let (shutdown, receiver) = watch::channel(false);
        let successful_connects = Arc::new(AtomicU64::new(0));
        let task = tokio::spawn(serve(
            listener,
            policy,
            receiver,
            successful_connects.clone(),
        ));
        Ok(Self {
            socket_path,
            shutdown,
            task: Some(task),
            successful_connects,
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn successful_connects(&self) -> u64 {
        self.successful_connects.load(Ordering::Relaxed)
    }

    /// Cancellation is joined before deleting the socket so an old proxy cannot
    /// accept a later attempt's traffic under the same path.
    pub async fn shutdown(mut self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            task.await.context("join egress proxy")?;
        }
        std::fs::remove_file(&self.socket_path).context("remove attempt egress socket")?;
        Ok(())
    }
}

async fn serve(
    listener: UnixListener,
    policy: EgressPolicy,
    mut shutdown: watch::Receiver<bool>,
    successful_connects: Arc<AtomicU64>,
) {
    let limit = Arc::new(Semaphore::new(policy.max_connections));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    return;
                }
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { return; };
                let Ok(permit) = limit.clone().try_acquire_owned() else {
                    let _ = reject(stream, "503 Too Many Connections").await;
                    continue;
                };
                let policy = policy.clone();
                let successful_connects = successful_connects.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = handle(stream, policy, successful_connects).await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}

async fn handle(
    mut client: UnixStream,
    policy: EgressPolicy,
    successful_connects: Arc<AtomicU64>,
) -> Result<()> {
    let target = match read_connect(&mut client).await {
        Ok(target) => target,
        Err(_) => return reject(client, "400 Bad Request").await,
    };
    if !policy.allows(&target.host) {
        return reject(client, "403 Forbidden").await;
    }
    let address = match resolve_public_target(&target.host).await {
        Ok(address) => address,
        Err(_) => return reject(client, "502 Bad Gateway").await,
    };
    let mut upstream = match timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(_)) | Err(_) => return reject(client, "502 Bad Gateway").await,
    };
    successful_connects.fetch_add(1, Ordering::Relaxed);
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

async fn reject(mut stream: UnixStream, status: &str) -> Result<()> {
    // Never include a target, DNS result, or credential reference in this
    // response: container logs are collected as workflow evidence.
    stream
        .write_all(format!("HTTP/1.1 {status}\r\nConnection: close\r\n\r\n").as_bytes())
        .await?;
    stream.shutdown().await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectTarget {
    host: String,
}

async fn read_connect(stream: &mut UnixStream) -> Result<ConnectTarget> {
    let mut bytes = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    while bytes.len() < MAX_REQUEST_BYTES {
        let read = stream.read(&mut byte).await?;
        if read == 0 {
            bail!("client closed before CONNECT request");
        }
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    ensure!(
        bytes.ends_with(b"\r\n\r\n"),
        "CONNECT request headers exceed limit"
    );
    let headers =
        std::str::from_utf8(&bytes).map_err(|_| anyhow!("CONNECT request is not UTF-8"))?;
    let request = headers.split("\r\n").next().unwrap_or_default();
    parse_connect_request(request)
}

fn parse_connect_request(request: &str) -> Result<ConnectTarget> {
    let mut pieces = request.split_ascii_whitespace();
    ensure!(
        pieces.next() == Some("CONNECT"),
        "only CONNECT requests are allowed"
    );
    let authority = pieces.next().context("CONNECT target missing")?;
    ensure!(
        matches!(pieces.next(), Some("HTTP/1.1") | Some("HTTP/1.0")) && pieces.next().is_none(),
        "CONNECT request line is malformed"
    );
    let (host, port) = authority
        .rsplit_once(':')
        .context("CONNECT target must include port 443")?;
    ensure!(port == "443", "only TLS port 443 is allowed");
    ensure!(
        !host.starts_with('[') && !host.contains(':') && !host.contains('/') && !host.contains('@'),
        "CONNECT target must be a DNS host"
    );
    Ok(ConnectTarget {
        host: validate_tls_domain(host)?,
    })
}

async fn resolve_public_target(host: &str) -> Result<SocketAddr> {
    let mut addresses = lookup_host((host, 443))
        .await
        .context("resolve egress host")?;
    let address = addresses
        .find(|address| public_ip(address.ip()))
        .context("DNS result contains no public address")?;
    Ok(address)
}

pub fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(value) => public_v4(value),
        IpAddr::V6(value) => public_v6(value),
    }
}

fn public_v4(value: Ipv4Addr) -> bool {
    let octets = value.octets();
    // Reject RFC1918, loopback, link-local, multicast, documentation and
    // other non-routable/reserved blocks instead of relying on DNS intent.
    if matches!(octets[0], 0 | 10 | 127 | 224..=255) {
        return false;
    }
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return false;
    }
    if octets[0] == 169 && octets[1] == 254 {
        return false;
    }
    if octets[0] == 172 && (16..=31).contains(&octets[1]) {
        return false;
    }
    if (octets[0], octets[1]) == (192, 0) || (octets[0], octets[1]) == (192, 168) {
        return false;
    }
    !matches!(
        octets,
        [198, 18 | 19, ..] | [198, 51, 100, ..] | [203, 0, 113, ..]
    )
}

fn public_v6(value: Ipv6Addr) -> bool {
    if value.is_loopback()
        || value.is_unspecified()
        || value.is_multicast()
        || value.to_ipv4_mapped().is_some()
    {
        return false;
    }
    let first = value.segments()[0];
    // fc00::/7 ULA and fe80::/10 link-local.
    (first & 0xfe00) != 0xfc00 && (first & 0xffc0) != 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_and_addresses_fail_closed() {
        assert!(parse_connect_request("CONNECT api.vendor.example:443 HTTP/1.1").is_ok());
        assert!(parse_connect_request("GET api.vendor.example:443 HTTP/1.1").is_err());
        assert!(parse_connect_request("CONNECT api.vendor.example:80 HTTP/1.1").is_err());
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_ip(address.parse().unwrap()), "{address}");
        }
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }
}
