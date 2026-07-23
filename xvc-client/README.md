# xvc-client

Async Rust client for [Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) servers.
It opens a TCP connection to a remote JTAG target and drives it through the three XVC operations: `get_info`, `set_tck`, and `shift`.

This crate is part of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) workspace.
It is typically used to script or automate JTAG operations against a running XVC server, or to stand in for Vivado (normally the client) in tests.

## Installation

```sh
cargo add xvc-client
```

The client is async and runs on a [Tokio](https://tokio.rs) runtime.

## Example

A runnable client is in [`examples/sample_client.rs`](https://github.com/Schottkyc137/xvc-rs/blob/main/xvc-client/examples/sample_client.rs).
Point it at any XVC server:

```sh
cargo run --example sample_client -- 127.0.0.1:2542
```
