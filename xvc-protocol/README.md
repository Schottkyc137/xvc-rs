# XVC Protocol Library

A Rust implementation of the [Xilinx Virtual Cable (XVC) 1.0 protocol](https://github.com/Xilinx/XilinxVirtualCable) for JTAG communication with FPGA devices over network connections.

## Features

- **Protocol Implementation**: Full XVC 1.0 support with message serialization/deserialization
- **Error Handling**: Robust parsing with detailed error reporting
- **Type Safety**: Leverages Rust's type system for protocol correctness

## Usage

See the [crate documentation](https://docs.rs/xvc-protocol/) for API documentation and usage examples.

### Quick Start

```rust
use xvc_protocol::{Message, XvcInfo};
use std::io::Cursor;

// Parse server capabilities
let response = b"xvcServer_v1.0:\x00\x00\xA0\x00\n";
let mut reader = Cursor::new(response);
let info = XvcInfo::from_reader(&mut reader)?;

// Send a message
let msg = Message::GetInfo;
let mut buffer = Vec::new();
msg.write_to(&mut buffer)?;
```
