# xvc-rs: Xilinx Virtual Cable in Rust

A Rust implementation of the [Xilinx Virtual Cable (XVC) 1.0 protocol](https://github.com/Xilinx/XilinxVirtualCable) for remote JTAG communication with FPGA devices over network connections.

## Disclaimer

This project is an independent implementation of an XVC server. Xilinx® is a registered trademark of AMD. This project is not affiliated with, endorsed by, or supported by AMD or Xilinx.

## Project Overview

xvc-rs is a modular, multi-crate Rust project providing both client and server implementations of the XVC protocol. It enables remote access to FPGA JTAG interfaces over network connections with multiple backend driver options.

## Crates

### [xvc-protocol](./xvc-protocol/)

The core protocol library implementing XVC 1.0 message serialization and deserialization.

- **Purpose**: Protocol definition and encoding/decoding for XVC messages
- **Key Features**: Full XVC 1.0 support with robust error handling and type-safe message handling

See [xvc-protocol README](./xvc-protocol/README.md) for detailed documentation.

### [xvc-client](./xvc-client/)

A client library for connecting to XVC servers and performing remote JTAG operations.

- **Purpose**: Provides a high-level API for clients to communicate with XVC servers
- **Key Features**: Simplified connection management and JTAG operation abstraction

See [xvc-client README](./xvc-client/README.md) for usage examples and API documentation.

### [xvc-server](./xvc-server/)

Core server library with pluggable backend architecture for JTAG hardware drivers.

- **Purpose**: Server-side protocol implementation with trait-based driver abstraction
- **Key Features**: Trait-based architecture for different hardware backends

See [xvc-server README](./xvc-server/README.md) for implementation details.

### [xvc-server-debugbridge](./xvc-server-debugbridge/)

Linux-specific backend implementations and ready-to-use command-line server binary.

- **Purpose**: Provides Linux drivers and a standalone server executable
- **Key Features**: Multiple driver backends (ioctl, UIO), command-line interface, logging support
- **Backends**:
  - **Ioctl Driver**: Kernel driver communication via ioctl syscalls
  - **UIO Driver**: Userspace I/O for memory-mapped FPGA interfaces

See [xvc-server-debugbridge README](./xvc-server-debugbridge/README.md) for command-line usage and environment configuration.

## Quick Start

### Client Usage

```rust
use xvc_client::XvcClient;

let mut client = XvcClient::new("127.0.0.1:2542")?;

// Query server capabilities
let info = client.get_info()?;
println!("Server version: {}", info.version());

// Set clock frequency
let actual_period = client.set_tck(10)?;

// Perform JTAG shift
let tdo = client.shift(8, vec![0x00], vec![0xA5])?;
println!("Received: {:?}", tdo);
```

### Server Usage

```rust
use xvc_server::{XvcServer, server::{Server, Config}};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

// Implement the trait for your hardware
struct MyDriver;

impl XvcServer for MyDriver {
    fn set_tck(&self, period_ns: u32) -> u32 { period_ns }
    fn shift(&self, _num_bits: u32, _tms: Box<[u8]>, tdi: Box<[u8]>) -> Box<[u8]> { tdi }
}

// Create and run the server
let driver = MyDriver;
let server = Server::new(driver, Config::default());
let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2542);
server.listen(addr)?;
```

### Command-line Server

```bash
# Start with ioctl driver
xvc-server-debugbridge --device kernel-driver

# Start with UIO driver
RUST_LOG=debug xvc-server-debugbridge --device uio-driver
```

## Project Structure

```
xvc-rs/
├── xvc-protocol/        # Core protocol implementation
├── xvc-client/          # Client library
├── xvc-server/          # Server library
└── xvc-server-debugbridge/    # Linux-specific drivers and CLI
```

## Building

This is a Rust workspace project using Cargo. Build all crates with:

```bash
cargo build
```

Or build specific crates:

```bash
cargo build -p xvc-protocol
cargo build -p xvc-client
cargo build -p xvc-server
cargo build -p xvc-server-debugbridge
```

## Cross compiling

Cross compilation is recommended through the usage of the [cross](https://github.com/cross-rs/cross) crate.
To build, simply use

```bash
cross build --target <target>
```

For example, to compile to a Zynqmp, use
```bash
cross build --target aarch64-unknown-linux-gnu
```

## Documentation

Generate and view documentation:

```bash
cargo doc --open
```

## Related Resources

- [Official Xilinx Virtual Cable](https://github.com/Xilinx/XilinxVirtualCable) - XVC protocol specification
