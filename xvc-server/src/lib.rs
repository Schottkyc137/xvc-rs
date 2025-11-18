//! # XVC Server Library
//!
//! This crate provides a foundation for implementing Xilinx Virtual Cable (XVC) servers
//! that handle JTAG communication with FPGA devices over network connections.
//!
//! ## Overview
//!
//! XVC is a protocol used by Xilinx design tools to interact with FPGA devices remotely.
//! This library abstracts the protocol handling and provides a server implementation that
//! can work with different backend device drivers.
//!
//! ## Architecture
//!
//! The crate is built around two main components:
//!
//! - **[`XvcServer`] Trait**: Defines the interface that backend drivers must implement
//!   to handle low-level JTAG operations (TCK configuration and vector shifting)
//! - **[`server::Server`]**: A generic server that handles XVC protocol communication,
//!   message parsing, and client connections
//!
//! ## How It Works
//!
//! 1. A backend driver (e.g., kernel driver, UIO device) implements the [`XvcServer`] trait
//! 2. The driver is wrapped in a [`server::Server`] instance
//! 3. The server listens for TCP connections and processes XVC protocol messages
//! 4. Each message is dispatched to the backend driver for actual JTAG operations
//! 5. Results are serialized and sent back to the client
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
//! ### Implementing a Backend Driver
//!
//! Create a struct that implements the [`XvcServer`] trait:
//!
//! ```ignore
//! use xvc_server::XvcServer;
//!
//! struct MyDriver {
//!     // device-specific fields
//! }
//!
//! impl XvcServer for MyDriver {
//!     fn set_tck(&self, period_ns: u32) -> u32 {
//!         // Configure hardware TCK period
//!         period_ns
//!     }
//!
//!     fn shift(&self, num_bits: u32, tms: Box<[u8]>, tdi: Box<[u8]>) -> Box<[u8]> {
//!         // Perform JTAG shifting and return TDO data
//!         Box::default()
//!     }
//! }
//! ```
//!
//! ### Starting the Server
//!
//! ```ignore
//! use xvc_server::server::{Server, Config};
//! use std::net::{IpAddr, Ipv4Addr, SocketAddr};
//!
//! let driver = MyDriver::new()?;
//! let config = Config::default();
//! let server = Server::new(driver, config);
//!
//! let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2542);
//! server.listen(addr)?;
//! ```
//!
//! ## Error Handling
//!
//! The XVC 1.0 protocol specification does not support error reporting in the Shift operation.
//! If a shift operation fails, an empty result is returned to the client.
//! For other operations, standard I/O errors are propagated as appropriate.
//!
//! ## Configuration
//!
//! Server behavior can be customized via [`server::Config`]:
//!
//! - **max_vector_size**: Maximum size of JTAG vectors (default: 10 MiB)
//! - **read_write_timeout**: Socket I/O timeout duration (default: 30 seconds)
//!
//! ## Logging
//!
//! This crate uses the `log` crate for diagnostics. Enable logging to see:
//! - Client connections and disconnections
//! - Protocol messages being processed
//! - Configuration details and error conditions
//!
//! Configure logging with an implementation like `env_logger`:
//!
//! ```ignore
//! env_logger::init();
//! ```
//!
//! ## Thread Model
//!
//! The server processes each client connection sequentially in a single thread.
//! For multi-client support, wrap the server in a multi-threaded framework or
//! run multiple server instances.
pub mod server;

/// Trait that backend drivers must implement to provide JTAG functionality.
///
/// This trait defines the interface between the XVC protocol server and the actual
/// hardware debug bridge driver. Implementors are responsible for translating
/// high-level JTAG operations into hardware-specific commands.
///
/// See the [`xvc-server-debugbridge`](https://docs.rs/xvc-server-debugbridge/) crate for examples.
pub trait XvcServer {
    /// Set the TCK (Test Clock) period.
    ///
    /// Configures the frequency of the JTAG Test Clock (TCK). The server attempts to set
    /// the requested period. If the hardware cannot achieve the exact period, it returns
    /// the closest achievable period.
    ///
    /// # Arguments
    ///
    /// * `period_ns` - The desired TCK period in nanoseconds
    ///
    /// # Returns
    ///
    /// The actual TCK period set by the hardware (in nanoseconds). This may differ from
    /// the requested value if the hardware has limited frequency resolution.
    fn set_tck(&self, period_ns: u32) -> u32;

    /// Shift JTAG TMS and TDI vectors into the device and return TDO data.
    ///
    /// Performs a JTAG shift operation by:
    /// 1. Shifting `tms` and `tdi` data into the JTAG chain
    /// 2. Capturing the corresponding `tdo` (Test Data Out) data
    /// 3. Returning the captured `tdo` data
    ///
    /// The operation is atomic with respect to the JTAG state machine.
    ///
    /// # Arguments
    ///
    /// * `num_bits` - Number of TCK cycles to perform
    /// * `tms` - Test Mode Select vector (must be ⌈num_bits / 8⌉ bytes)
    /// * `tdi` - Test Data In vector (must be ⌈num_bits / 8⌉ bytes)
    ///
    /// # Returns
    ///
    /// Test Data Out vector of the same size as `tms` and `tdi`. On error,
    /// an empty vector should be returned.
    ///
    /// # Error Handling
    ///
    /// The XVC 1.0 protocol does not support error reporting for shift operations.
    /// Implementations should return an empty box on error rather than propagating errors.
    fn shift(&self, num_bits: u32, tms: Box<[u8]>, tdi: Box<[u8]>) -> Box<[u8]>;
}
