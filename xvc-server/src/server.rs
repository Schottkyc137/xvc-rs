use std::{io, sync::Arc, time::Duration};

use bytes::BytesMut;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, ToSocketAddrs, tcp::OwnedReadHalf},
    sync::Mutex,
    task::block_in_place,
    time::timeout,
};
use tokio_util::codec::Decoder;
use tokio_util::sync::CancellationToken;

use crate::XvcServer;
use xvc_protocol::{
    Message, OwnedMessage, Version, XvcInfo, error::ReadError, tokio_codec::MessageDecoder,
};

#[derive(Debug, Clone)]
pub struct Config {
    /// Maximum JTAG vector size in bytes that the server will accept (default: 10 MiB).
    pub max_vector_size: u32,
    /// Timeout applied to each TCP read. Connections that are idle for longer than
    /// this duration are closed (default: 30 s).
    pub read_write_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_vector_size: 10 * 1024 * 1024,
            read_write_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
pub struct Server<T: XvcServer> {
    server: Arc<Mutex<T>>,
    config: Config,
}

/// Builder to create a [Server] instance and modify configuration options
///
/// # Example
///
/// ```ignore
/// use xvc_server::server::Builder;
/// use std::time::Duration;
///
/// let server = Builder::new()
///     .max_vector_size(1024)
///     .rw_timeout(Duration::from_secs(20))
///     .build(my_server);
/// ```
#[derive(Default)]
pub struct Builder {
    config: Config,
}

impl Builder {
    pub fn new() -> Builder {
        Builder::default()
    }

    /// Set the highest vector size that this server is expected to receive.
    pub fn max_vector_size(mut self, size: u32) -> Self {
        self.config.max_vector_size = size;
        self
    }

    /// Set the TCP read timeout applied to each message receive.
    pub fn rw_timeout(mut self, timeout: Duration) -> Self {
        self.config.read_write_timeout = timeout;
        self
    }

    /// Build and return the server.
    pub fn build<T: XvcServer>(self, server: T) -> Server<T> {
        Server::new(server, self.config)
    }
}

impl<T: XvcServer> Server<T> {
    /// Create a new server wrapping `server` with the given `config`.
    pub fn new(server: T, config: Config) -> Server<T> {
        Server {
            server: Arc::new(Mutex::new(server)),
            config,
        }
    }

    /// Bind to `addr` and serve clients until the process exits.
    ///
    /// This is the standard production entry point. To shut the server down
    /// programmatically (e.g. in tests), use [`listen_on`](Self::listen_on)
    /// with a [`CancellationToken`].
    pub async fn listen(&self, addr: impl ToSocketAddrs) -> io::Result<()>
    where
        T: Send + 'static,
    {
        let listener = TcpListener::bind(addr).await?;
        self.listen_on(listener, CancellationToken::new()).await
    }

    /// Serve clients from a pre-bound `listener` until `shutdown` is cancelled.
    ///
    /// When `shutdown` is cancelled the accept loop exits cleanly; any connection
    /// that is already being served runs to completion before the task finishes.
    ///
    /// This entry point is useful when the caller needs to control the server
    /// lifetime programmatically — for example in tests, or to hook into a
    /// process-wide signal handler:
    ///
    /// ```ignore
    /// let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    /// let addr = listener.local_addr()?;
    /// let token = CancellationToken::new();
    ///
    /// // Shut down on Ctrl+C
    /// tokio::spawn({
    ///     let token = token.clone();
    ///     async move {
    ///         tokio::signal::ctrl_c().await.unwrap();
    ///         token.cancel();
    ///     }
    /// });
    ///
    /// server.listen_on(listener, token).await?;
    /// ```
    pub async fn listen_on(
        &self,
        listener: TcpListener,
        shutdown: CancellationToken,
    ) -> io::Result<()>
    where
        T: Send + 'static,
    {
        log::info!("Server listening for connections");

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    log::info!("Shutdown signal received, stopping listener");
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let guard = match Arc::clone(&self.server).try_lock_owned() {
                                Ok(guard) => guard,
                                Err(_) => {
                                    log::warn!("Rejected concurrent client from {}: another client is already active", addr);
                                    continue;
                                }
                            };
                            log::info!("New client connection from {}", addr);
                            let config = self.config.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_client(guard, config, stream).await {
                                    log::error!("Client error: {}", e);
                                }
                            });
                        }
                        Err(e) => log::error!("Connection error: {}", e),
                    }
                }
            }
        }

        Ok(())
    }
}

async fn handle_client<T>(
    server: tokio::sync::OwnedMutexGuard<T>,
    config: Config,
    stream: TcpStream,
) -> Result<(), ReadError>
where
    T: XvcServer + Send + 'static,
{
    let (mut read_half, mut write_half) = stream.into_split();
    let mut buf = BytesMut::new();
    let mut decoder = MessageDecoder::new(config.max_vector_size as usize);

    loop {
        match read_message(
            &mut read_half,
            &mut buf,
            &mut decoder,
            config.read_write_timeout,
        )
        .await
        {
            Ok(Some(msg)) => {
                let response = block_in_place(|| compute_response(&*server, &config, msg))?;
                write_half.write_all(&response).await?;
            }
            Ok(None) => break,
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

/// Read one complete message from `read`, respecting `rw_timeout` per read call.
/// Returns `Ok(None)` on clean EOF or timeout.
async fn read_message(
    read: &mut OwnedReadHalf,
    buf: &mut BytesMut,
    decoder: &mut MessageDecoder,
    rw_timeout: Duration,
) -> Result<Option<OwnedMessage>, ReadError> {
    loop {
        if let Some(msg) = decoder.decode(buf)? {
            return Ok(Some(msg));
        }

        match timeout(rw_timeout, read.read_buf(buf)).await {
            Ok(Ok(0)) => return Ok(None), // clean EOF
            Ok(Ok(_)) => {}               // more bytes, loop and try to decode
            Ok(Err(e)) => return Err(ReadError::from(e)),
            Err(_elapsed) => {
                log::warn!("Client read timeout, closing connection");
                return Ok(None);
            }
        }
    }
}

fn compute_response<T: XvcServer>(
    server: &T,
    config: &Config,
    msg: OwnedMessage,
) -> Result<Vec<u8>, ReadError> {
    let mut buf = Vec::new();
    match msg {
        Message::GetInfo => {
            log::info!("Received GetInfo message");
            let info = XvcInfo::new(Version::V1_0, config.max_vector_size);
            info.write_to(&mut buf)?;
            log::debug!("Sent XVC info response");
        }
        Message::SetTck { period_ns } => {
            log::debug!("Received SetTck message: period_ns={}", period_ns);
            let ret_period = server.set_tck(period_ns);
            log::debug!("Set TCK returned: period_ns={}", ret_period);
            buf.extend_from_slice(&ret_period.to_le_bytes());
        }
        Message::Shift { num_bits, tms, tdi } => {
            log::debug!(
                "Received Shift message: num_bits={}, tms_len={}, tdi_len={}",
                num_bits,
                tms.len(),
                tdi.len()
            );
            log::trace!("Shift TMS data: {:02x?}", &tms[..]);
            log::trace!("Shift TDI data: {:02x?}", &tdi[..]);
            let tdo = server.shift(num_bits, &tms, &tdi);
            log::trace!("Shift result TDO data: {:02x?}", &tdo[..]);
            buf.extend_from_slice(&tdo);
        }
    }
    Ok(buf)
}
