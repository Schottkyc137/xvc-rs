# xvc-protocol

Core implementation of the [Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) wire format: the message types and codec implementation that serializes and deserializes them.

This crate is the foundation of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) workspace; all other crates build on it directly or indirectly.
Depend on it directly only when writing a custom client, server, or tooling that speaks XVC — otherwise reach for the higher-level [`xvc-server`](https://crates.io/crates/xvc-server) or [`xvc-client`](https://crates.io/crates/xvc-client) instead.

## Installation

```sh
cargo add xvc-protocol
```

### Tokio support

Enable the `tokio` feature for async codecs built on [`tokio-util`](https://docs.rs/tokio-util):

```sh
cargo add xvc-protocol --features tokio
```
