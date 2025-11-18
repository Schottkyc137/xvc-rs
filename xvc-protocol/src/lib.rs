//! # XVC Protocol Library
//!
//! This crate provides a Rust implementation of the [Xilinx Virtual Cable (XVC)](https://github.com/Xilinx/XilinxVirtualCable) protocol,
//! enabling client-server communication for JTAG vector shifting and cable configuration.
//!
//! ## Overview
//!
//! XVC is a protocol used by Xilinx design tools to interact with FPGA devices over a network connection.
//! This library implements the protocol specification, allowing you to:
//!
//! - Serialize and deserialize XVC protocol messages
//! - Exchange JTAG vectors with an XVC server
//! - Configure TCK (Test Clock) timing parameters
//! - Query server capabilities and protocol version information
//!
//! ## Protocol Features
//!
//! - **Protocol Versions**: XVC 1.0
//! - **Message Types**:
//!   - `GetInfo`: Query server capabilities (protocol version, max vector length)
//!   - `SetTck`: Configure the TCK clock period in nanoseconds
//!   - `Shift`: Shift JTAG TMS/TDI vectors into a device
//!
//! ## Basic Usage
//!
//! ### Reading Messages from a Server
//!
//! ```
//! use xvc_protocol::{Message, XvcInfo, Version};
//! use std::io::Cursor;
//!
//! // Read server capabilities
//! let server_response = b"xvcServer_v1.0:32\n";
//! let mut reader = Cursor::new(server_response);
//! let info = XvcInfo::from_reader(&mut reader).expect("Info should parse");
//! assert_eq!(info.version(), Version::V1_0);
//! assert_eq!(info.max_vector_len(), 32);
//! ```
//!
//! ### Writing Messages to a Server
//!
//! ```
//! use xvc_protocol::Message;
//! use std::vec::Vec;
//!
//! // Request server info
//! let msg = Message::GetInfo;
//! let mut buffer = Vec::new();
//! msg.write_to(&mut buffer).expect("Writing to vector shouldn't fail");
//! // Send buffer to server...
//! assert_eq!(buffer, b"getinfo:");
//! ```
//!
//! ### Shifting JTAG Vectors
//!
//! ```
//! use xvc_protocol::Message;
//!
//! let num_bytes = 2;
//! let tms = vec![0xAA; num_bytes].into_boxed_slice();
//! let tdi = vec![0x55; num_bytes].into_boxed_slice();
//!
//! let shift_msg = Message::Shift { num_bits: 2 * num_bytes as u32, tms, tdi };
//! let mut output = Vec::new();
//! shift_msg.write_to(&mut output).expect("Writing to vector shouldn't fail");
//! assert_eq!(output, b"shift:\x04\x00\x00\x00\xAA\xAA\x55\x55");
//! ```
//!
//! ## Message Format
//!
//! All messages use a binary protocol with the following structure:
//!
//! - **GetInfo**: `getinfo:`
//! - **SetTck**: `settck:<period in ns: u32>`
//! - **Shift**: `shift:<num_bits: u32><TMS vector><TDI vector>`
//! - **XvcInfo**: `xvcServer_v{version}:<max_vector_len: u32>\n`
//!
//! ## Error Handling
//!
//! This library uses the [`error::ReadError`] type for protocol parsing errors.
//!
//! ## Thread Safety
//!
//! The types in this library are thread-safe and can be safely shared across threads.
//! However, I/O operations (reading/writing) are not synchronized and require external coordination.

pub mod protocol;
pub use protocol::*;
pub mod codec;
pub mod error;
