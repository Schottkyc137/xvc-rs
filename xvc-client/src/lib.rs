//! # XVC Client
//!
//! A Rust client library for connecting to Xilinx Virtual Cable (XVC) servers
//! and performing remote JTAG operations on FPGA devices.
//!
//! ## Overview
//!
//! This crate provides a high-level client interface to XVC servers, allowing applications
//! to interact with FPGA debug interfaces over network connections. It handles protocol
//! communication, message serialization, and provides convenient methods for JTAG operations.
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
//! use std::net::SocketAddr;
//!
//! let mut client = XvcClient::connect("127.0.0.1:2542")?;
//!
//! // Query server capabilities
//! let info = client.get_info()?;
//! println!("Server version: {}", info.version());
//! println!("Max vector size: {} bytes", info.max_vector_size());
//! ```
//!
//! ### Setting Clock Frequency
//!
//! ```ignore
//! // Set TCK period to 10 nanoseconds
//! let actual_period = client.set_tck(10)?;
//! println!("Set TCK to {} ns", actual_period);
//! ```
//!
//! ### Performing JTAG Shifts
//!
//! ```ignore
//! // Perform a 8-bit JTAG shift
//! let num_bits = 8;
//! let tms = vec![0x00]; // Test Mode Select vector
//! let tdi = vec![0xA5]; // Test Data In vector
//!
//! let tdo = client.shift(num_bits, tms, tdi)?;
//! println!("TDO data: {:?}", tdo);
//! ```
//!
//! ## Related Crates
//!
//! - [`xvc_server`](https://docs.rs/xvc-server/) - Server implementation
//! - [`xvc_protocol`](https://docs.rs/xvc-protocol/) - Protocol encoding/decoding
//! - [`xvc_server_linux`](https://docs.rs/xvc-server-debugbridge/) - Linux server drivers
use std::{
    io::{self, Read},
    net::{TcpStream, ToSocketAddrs},
};

use xvc_protocol::{Message, XvcInfo, error::ReadError};

/// XVC client for remote JTAG operations.
///
/// Connects to an XVC server and provides methods for JTAG operations.
pub struct XvcClient {
    tcp: TcpStream,
}

impl XvcClient {
    pub fn new(addr: impl ToSocketAddrs) -> io::Result<XvcClient> {
        Ok(XvcClient {
            tcp: TcpStream::connect(addr)?,
        })
    }

    /// Query server capabilities and version information.
    pub fn get_info(&mut self) -> Result<XvcInfo, ReadError> {
        Message::GetInfo.write_to(&mut self.tcp)?;
        XvcInfo::from_reader(&mut self.tcp)
    }

    /// Set the JTAG Test Clock (TCK) period.
    /// # Returns
    ///
    /// The actual TCK period set by the server.
    // May differ from requested, if the server does not support the requested rate.
    pub fn set_tck(&mut self, period_ns: u32) -> io::Result<u32> {
        Message::SetTck { period_ns }.write_to(&mut self.tcp)?;
        let mut buf = [0u8; 4];
        self.tcp.read_exact(&mut buf)?;
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
    pub fn shift(&mut self, num_bits: u32, tms: &[u8], tdi: &[u8]) -> io::Result<Box<[u8]>> {
        Message::Shift {
            num_bits,
            tms: tms.into(),
            tdi: tdi.into(),
        }
        .write_to(&mut self.tcp)?;
        let mut buf = vec![0; num_bits.div_ceil(8) as usize];
        self.tcp.read_exact(&mut buf)?;
        Ok(buf.into_boxed_slice())
    }
}
