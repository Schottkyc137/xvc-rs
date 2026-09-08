# xvc-protocol

Core implementation of the [Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) wire format.
This library contains message types and serialization / deserialization facilities.

The crate is the foundation of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) project; all other crates build on it directly or indirectly.
Unless there is a specific reason not to, authors of rust-based tooling for XVC should depend on the higher-level [`xvc-server`](https://crates.io/crates/xvc-server) or [`xvc-client`](https://crates.io/crates/xvc-client) crates.

## Installation

```sh
cargo add xvc-protocol
```

### Cargo features

**Tokio Support**

The `tokio` feature enables async codecs built on [`tokio-util`](https://docs.rs/tokio-util) for integration with the [`tokio`](https://tokio.rs) framework:

```sh
cargo add xvc-protocol --features tokio
```
