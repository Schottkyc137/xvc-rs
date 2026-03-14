//! # XVC Client
//!
//! A Rust client library for connecting to Xilinx Virtual Cable (XVC) servers
//! and performing remote JTAG operations on FPGA devices.
//!
//! ## Overview
//!
//! This crate provides a high-level async client interface to XVC servers, allowing
//! applications to interact with FPGA debug interfaces over network connections.
//! It handles protocol communication, message serialization, and provides convenient
//! methods for JTAG operations.
//!
//! ## Protocol Support
//!
//! This implementation supports the XVC 1.0 protocol with the following operations:
//!
//! - **GetInfo**: Query server capabilities (version, max vector size)
//! - **SetTck**: Configure the JTAG Test Clock (TCK) period
//! - **Shift**: Perform JTAG vector shifting (TMS/TDI/TDO)
//!
//! For detailed protocol information, see the [`xvc_protocol`](https://docs.rs/xvc-protocol/) crate.
//!
//! ## Basic Usage
//!
//! ### Connecting to a Server
//!
//! ```ignore
//! use xvc_client::XvcClient;
//!
//! let mut client = XvcClient::connect("127.0.0.1:2542").await?;
//!
//! // Query server capabilities
//! let info = client.get_info().await?;
//! println!("Server version: {}", info.version());
//! println!("Max vector size: {} bytes", info.max_vector_len());
//! ```
//!
//! ### Setting Clock Frequency
//!
//! ```ignore
//! // Set TCK period to 10 nanoseconds
//! let actual_period = client.set_tck(10).await?;
//! println!("Set TCK to {} ns", actual_period);
//! ```
//!
//! ### Performing JTAG Shifts
//!
//! ```ignore
//! // Perform an 8-bit JTAG shift
//! let num_bits = 8;
//! let tms = vec![0x00];
//! let tdi = vec![0xA5];
//!
//! let tdo = client.shift(num_bits, tms.into_boxed_slice(), tdi.into_boxed_slice()).await?;
//! println!("TDO data: {:?}", tdo);
//! ```
//!
//! ## Related Crates
//!
//! - [`xvc_server`](https://docs.rs/xvc-server/) - Server implementation
//! - [`xvc_protocol`](https://docs.rs/xvc-protocol/) - Protocol encoding/decoding
//! - [`xvc_server_linux`](https://docs.rs/xvc-server-debugbridge/) - Linux server drivers
use std::io;

use bytes::BytesMut;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs},
};
use tokio_util::codec::Decoder;

use xvc_protocol::{Message, XvcInfo, error::ReadError, tokio_codec::XvcInfoDecoder};

/// XVC client for remote JTAG operations.
///
/// Connects to an XVC server and provides async methods for JTAG operations.
/// All methods share a single persistent TCP connection.
pub struct XvcClient {
    tcp: TcpStream,
}

impl XvcClient {
    /// Connect to an XVC server at `addr`.
    pub async fn connect(addr: impl ToSocketAddrs) -> io::Result<XvcClient> {
        Ok(XvcClient {
            tcp: TcpStream::connect(addr).await?,
        })
    }

    /// Query server capabilities and version information.
    pub async fn get_info(&mut self) -> Result<XvcInfo, ReadError> {
        self.write_message(Message::GetInfo).await?;

        let mut buf = BytesMut::new();
        loop {
            match XvcInfoDecoder.decode(&mut buf)? {
                Some(info) => return Ok(info),
                None => {
                    if self.tcp.read_buf(&mut buf).await? == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "connection closed while reading server info",
                        )
                        .into());
                    }
                }
            }
        }
    }

    /// Set the JTAG Test Clock (TCK) period.
    ///
    /// Returns the actual period set by the server, which may differ from the
    /// requested value if the hardware has limited frequency resolution.
    pub async fn set_tck(&mut self, period_ns: u32) -> Result<u32, ReadError> {
        self.write_message(Message::SetTck { period_ns }).await?;
        let mut buf = [0u8; 4];
        self.tcp.read_exact(&mut buf).await?;
        Ok(u32::from_le_bytes(buf))
    }

    /// Perform a JTAG shift operation.
    ///
    /// # Arguments
    ///
    /// * `num_bits` - Number of bits to shift
    /// * `tms` - Test Mode Select vector (length must be ⌈num_bits / 8⌉)
    /// * `tdi` - Test Data In vector (length must be ⌈num_bits / 8⌉)
    ///
    /// # Returns
    ///
    /// Test Data Out vector from the JTAG chain of the same length as `tms` and `tdi`.
    pub async fn shift(
        &mut self,
        num_bits: u32,
        tms: Box<[u8]>,
        tdi: Box<[u8]>,
    ) -> Result<Box<[u8]>, ReadError> {
        self.write_message(Message::Shift { num_bits, tms, tdi })
            .await?;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let mut buf = vec![0u8; num_bytes];
        self.tcp.read_exact(&mut buf).await?;
        Ok(buf.into_boxed_slice())
    }

    async fn write_message(&mut self, msg: Message) -> Result<(), ReadError> {
        let mut buf = Vec::new();
        msg.write_to(&mut buf)?;
        self.tcp.write_all(&buf).await?;
        Ok(())
    }
}
