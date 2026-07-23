# xvc-server

Library for building [Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) servers.
It handles the protocol, TCP connections, and client management; library users implement the `XvcServer` trait to drive specific JTAG hardware.

This crate is part of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) workspace and is the extension point for new hardware backends.
For ready-to-run servers, see [`xvc-server-debugbridge`](https://crates.io/crates/xvc-server-debugbridge) (Linux debug bridges) and [`xvc-server-usb`](https://crates.io/crates/xvc-server-usb) (FTDI USB-to-JTAG adapters).

## Installation

```sh
cargo add xvc-server
```

The server is async and requires a multi-threaded [Tokio](https://tokio.rs) runtime.

## Example

A minimal loopback server is in [`examples/mock_server.rs`](https://github.com/Schottkyc137/xvc-rs/blob/main/xvc-server/examples/mock_server.rs).
Start it, then point any XVC client at it:

```sh
cargo run --example mock_server
```
