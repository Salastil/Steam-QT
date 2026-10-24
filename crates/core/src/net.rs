//! WebSocket transport to Steam's connection managers.
//!
//! Both protocol stacks we build on (steam-vent for the long-lived session, steamroom for the
//! login handshake) accept a caller-supplied transport. This module provides one that honours
//! `HTTPS_PROXY`/`NO_PROXY` and trusts the system certificate store, then adapts it to each
//! library's interface.

use bytes::{Bytes, BytesMut};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tracing::debug;

pub type WsStream = WebSocketStream<TlsStream<TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("invalid url: {0}")]
    Url(String),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("proxy refused CONNECT: {0}")]
    Proxy(String),
    #[error("websocket: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
}

/// Shared TLS configuration: system trust store plus Mozilla roots as a fallback.
pub fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("ring supports the default protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

/// The proxy to use for `host`, if any, as `host:port`.
fn proxy_for(host: &str) -> Option<String> {
    let proxy = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .ok()
        .filter(|p| !p.is_empty())?;
    let no_proxy = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    let bypass = no_proxy.split(',').map(str::trim).any(|entry| {
        let entry = entry.trim_start_matches('*');
        !entry.is_empty() && (host == entry.trim_start_matches('.') || host.ends_with(entry))
    });
    if bypass {
        return None;
    }
    let authority = proxy
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    Some(authority.to_string())
}

async fn open_tcp(host: &str, port: u16) -> Result<TcpStream, NetError> {
    let Some(proxy) = proxy_for(host) else {
        return Ok(TcpStream::connect((host, port)).await?);
    };
    debug!(%proxy, host, port, "connecting through proxy");
    let mut stream = TcpStream::connect(&proxy).await?;
    let request = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut reader = BufReader::new(&mut stream);
    let mut status = String::new();
    reader.read_line(&mut status).await?;
    if !status
        .split_whitespace()
        .nth(1)
        .is_some_and(|code| code == "200")
    {
        return Err(NetError::Proxy(status.trim().to_string()));
    }
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 || line == "\r\n" {
            break;
        }
    }
    Ok(stream)
}

/// Open a `wss://` connection.
pub async fn connect_ws(url: &str) -> Result<WsStream, NetError> {
    let parsed = url::Url::parse(url).map_err(|e| NetError::Url(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| NetError::Url(url.to_string()))?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);

    let tcp = open_tcp(&host, port).await?;
    let server_name = rustls::pki_types::ServerName::try_from(host.clone())
        .map_err(|e| NetError::Url(e.to_string()))?;
    let tls = TlsConnector::from(tls_config())
        .connect(server_name, tcp)
        .await?;
    let (ws, _) = tokio_tungstenite::client_async(url, tls).await?;
    debug!(url, "websocket connected");
    Ok(ws)
}

/// URL of a connection manager's WebSocket endpoint, from a `host:port` endpoint string.
pub fn cm_url(endpoint: &str) -> String {
    format!("wss://{endpoint}/cmsocket/")
}

/// Split a WebSocket into the sink/stream pair steam-vent's
/// `UnAuthenticatedConnection::from_sender_receiver` expects.
#[allow(clippy::type_complexity)]
pub fn vent_pair(
    ws: WsStream,
) -> (
    impl Sink<BytesMut, Error = steam_vent::NetworkError> + Send + 'static,
    impl Stream<Item = Result<BytesMut, steam_vent::NetworkError>> + Send + 'static,
) {
    let (write, read) = ws.split();
    let sink = write
        .sink_map_err(steam_vent::NetworkError::from)
        .with(|msg: BytesMut| ready(Ok(Message::binary(msg.freeze()))));
    let stream = read
        .try_filter(|m| ready(m.is_binary()))
        .map_err(steam_vent::NetworkError::from)
        .map_ok(|m| BytesMut::from(&m.into_data()[..]));
    (sink, stream)
}

/// steamroom transport over our WebSocket.
pub struct RoomTransport {
    sink: Mutex<SplitSink<WsStream, Message>>,
    stream: Mutex<SplitStream<WsStream>>,
}

impl RoomTransport {
    pub fn new(ws: WsStream) -> Self {
        let (sink, stream) = ws.split();
        Self {
            sink: Mutex::new(sink),
            stream: Mutex::new(stream),
        }
    }
}

fn room_io(e: impl std::error::Error + Send + Sync + 'static) -> steamroom::error::Error {
    steamroom::error::ConnectionError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))
        .into()
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

impl steamroom::transport::Transport for RoomTransport {
    fn send(&self, payload: &[u8]) -> BoxFuture<'_, Result<(), steamroom::error::Error>> {
        let msg = Message::binary(Bytes::copy_from_slice(payload));
        Box::pin(async move { self.sink.lock().await.send(msg).await.map_err(room_io) })
    }

    fn recv(&self) -> BoxFuture<'_, Result<Bytes, steamroom::error::Error>> {
        Box::pin(async move {
            loop {
                let msg = self
                    .stream
                    .lock()
                    .await
                    .next()
                    .await
                    .ok_or(steamroom::error::ConnectionError::Disconnected)?
                    .map_err(room_io)?;
                match msg {
                    Message::Binary(data) => return Ok(data),
                    Message::Close(_) => {
                        return Err(steamroom::error::ConnectionError::Disconnected.into());
                    }
                    _ => continue,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_proxy_matching() {
        // SAFETY: tests in this module run single-threaded with respect to these variables.
        unsafe {
            std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:3128");
            std::env::set_var("NO_PROXY", "localhost,.internal,*.lan");
        }
        assert_eq!(
            proxy_for("cm1.steamserver.net").as_deref(),
            Some("127.0.0.1:3128")
        );
        assert_eq!(proxy_for("localhost"), None);
        assert_eq!(proxy_for("box.internal"), None);
        assert_eq!(proxy_for("nas.lan"), None);
    }
}
