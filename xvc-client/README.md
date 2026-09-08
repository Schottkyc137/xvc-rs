# xvc-client

Async Rust client for [Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) servers.
This crate offers the `XvcClient` struct to communicate with XVC servers and, consequently, with the connected JTAG device.

This crate is part of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) project.
It is typically used to script or automate JTAG operations against a running XVC server, or to stand in for AMD/Xilinx tools in tests.

## Installation

```sh
cargo add xvc-client
```

The client is async and runs on a [tokio](https://tokio.rs) runtime.

## Example

A runnable client is in [`examples/sample_client.rs`](https://github.com/Schottkyc137/xvc-rs/blob/main/xvc-client/examples/sample_client.rs).
Point it at any XVC server:

```sh
cargo run --example sample_client -- <ip>:<port>
```
