use std::{
    io::{ErrorKind, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    time::Duration,
};

use crate::XvcServer;
use xvc_protocol::error::ReadError;
use xvc_protocol::{Message, Version, XvcInfo};

#[derive(Debug, Clone)]
pub struct Config {
    pub max_vector_size: u32,
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
    server: T,
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

    /// Set the TCP read and write timeout
    pub fn rw_timeout(mut self, timeout: Duration) -> Self {
        self.config.read_write_timeout = timeout;
        self
    }

    /// Build and return the server
    pub fn build<T: XvcServer>(self, server: T) -> Server<T> {
        Server::new(server, self.config)
    }
}

impl<T: XvcServer> Server<T> {
    pub fn new(server: T, config: Config) -> Server<T> {
        Server { server, config }
    }

    pub fn listen(&self, addr: impl ToSocketAddrs) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr)?;
        log::info!("Server listening for connections");

        for stream in listener.incoming() {
            match stream {
                Ok(tcp) => {
                    let peer_addr = tcp.peer_addr().ok();
                    if let Some(addr) = peer_addr {
                        log::info!("New client connection from {}", addr);
                    }
                    if let Err(e) = self.handle_client(tcp) {
                        log::error!("Client error: {}", e);
                    }
                }
                Err(e) => log::error!("Connection error: {}", e),
            }
        }
        Ok(())
    }

    fn handle_client(&self, mut tcp: TcpStream) -> Result<(), ReadError> {
        tcp.set_read_timeout(Some(self.config.read_write_timeout))?;
        tcp.set_write_timeout(Some(self.config.read_write_timeout))?;

        loop {
            match Message::from_reader(&mut tcp, self.config.max_vector_size as usize) {
                Ok(message) => self.process_message(message, &mut tcp)?,
                Err(ReadError::IoError(err)) if err.kind() == ErrorKind::TimedOut => {
                    log::error!("Client read timeout, closing connection");
                    break;
                }
                Err(ReadError::IoError(err))
                    if err.kind() == ErrorKind::ConnectionAborted
                        || err.kind() == ErrorKind::ConnectionReset =>
                {
                    break;
                } // Client disconnected
                Err(other) => return Err(other),
            }
        }
        Ok(())
    }

    /// Process each message, forwarding the implementation to the server.
    fn process_message(&self, message: Message, tcp: &mut TcpStream) -> Result<(), ReadError> {
        match message {
            Message::GetInfo => {
                log::info!("Received GetInfo message");
                let info = XvcInfo::new(Version::V1_0, self.config.max_vector_size);
                info.write_to(tcp)?;
                log::debug!("Sent XVC info response");
            }
            Message::SetTck { period_ns } => {
                log::debug!("Received SetTck message: period_ns={}", period_ns);
                let ret_period = self.server.set_tck(period_ns);
                log::debug!("Set TCK returned: period_ns={}", ret_period);
                tcp.write_all(&ret_period.to_le_bytes())?;
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
                let tdo = self.server.shift(num_bits, tms, tdi);
                log::trace!("Shift result TDO data: {:02x?}", &tdo[..]);
                tcp.write_all(&tdo)?;
            }
        }
        Ok(())
    }
}
