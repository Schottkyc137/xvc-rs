# XVC Client

A Rust client library for connecting to Xilinx Virtual Cable (XVC) servers and performing remote JTAG operations.

## Quick Start

### Basic Connection and Operation

```rust
use xvc_client::XvcClient;

let mut client = XvcClient::new("127.0.0.1:2542")?;

// Query server capabilities
let info = client.get_info()?;
println!("Server version: {}", info.version());

// Set clock frequency
let actual_period = client.set_tck(10)?; // 10 ns

// Perform JTAG shift
let tdo = client.shift(8, vec![0x00], vec![0xA5])?;
println!("Received: {:?}", tdo);
```

## Usage

See the [crate documentation](https://docs.rs/xvc-client/) for API documentation and usage examples.

## See Also

- [xvc-server](../xvc-server/) - Server implementation
- [xvc-server-debugbridge](../xvc-server-debugbridge/) - Linux-specific drivers
- [xvc-protocol](../xvc-protocol/) - Protocol encoding/decoding
- [Xilinx Virtual Cable](https://github.com/Xilinx/XilinxVirtualCable) - Official XVC specification