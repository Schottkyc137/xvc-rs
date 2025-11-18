# XVC Server

A Rust library for implementing Xilinx Virtual Cable (XVC) servers that handle JTAG communication with FPGA devices over network connections.

## Features

- **Protocol Implementation**: Full XVC 1.0 support for remote JTAG operations
- **Pluggable Backends**: Trait-based architecture for different hardware drivers

## Quick Start

### Minimal Example

```rust
use xvc_server::{XvcServer, server::{Server, Config}};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

// Implement the trait for your hardware
struct MyDriver;

impl XvcServer for MyDriver {
    fn set_tck(&self, period_ns: u32) -> u32 {
        period_ns
    }

    fn shift(&self, _num_bits: u32, _tms: Box<[u8]>, tdi: Box<[u8]>) -> Box<[u8]> {
        tdi
    }
}

// Create and run the server
let driver = MyDriver;
let config = Config::default();
let server = Server::new(driver, config);

let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2542);
server.listen(addr)?;
```

## Usage

See the [crate documentation](https://docs.rs/xvc-server/) for detailed documentation.

## See Also

- [xvc-protocol](../xvc-protocol/) - Protocol encoding/decoding
- [xvc-server-debugbridge](../xvc-server-debugbridge/) - Linux-specific driver implementations
- [Xilinx Virtual Cable](https://github.com/Xilinx/XilinxVirtualCable) - Official XVC specification
