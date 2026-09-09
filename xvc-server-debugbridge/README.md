# xvc-server-debugbridge

Ready-to-run [Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) server for [AMD Debug Bridges](https://www.amd.com/en/products/adaptive-socs-and-fpgas/intellectual-property/debug-bridge.html) on Linux.
It provides the `xvc-bridge` binary, which exposes a Debug Bridge instantiated in your FPGA design over TCP so tools like Vivado can reach it.

This crate is part of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) project and builds on [`xvc-server`](https://crates.io/crates/xvc-server).

## Requirements

- The SoC must run Linux; bare-metal or other operating systems are not supported. If you believe to have a use case for those, please open a [feature request](https://github.com/Schottkyc137/xvc-rs/issues/new).
- A [Debug Bridge](https://docs.amd.com/v/u/en-US/pg245-debug-bridge) (or an equivalent AXI-to-JTAG bridge) instantiated on the target FPGA.
- Permission to access the chosen device node, typically `root`. Depending on the backend, this is one of `/dev/xilinx_xvc_driver`, `/dev/uioN`, or `/dev/mem`.

## Installation

The target SoC often has no public internet access, so a ready-to-run binary for AMD SoCs (aarch64 Linux) is available on the [releases page](https://github.com/Schottkyc137/xvc-rs/releases?q=xvc-server-debugbridge&expanded=true).
Download it, copy it to the SoC, and run it there.

### Alternative installations

If the target has internet access, install it directly with cargo:

```sh
cargo install xvc-server-debugbridge
```

The server usually runs on the SoC's ARM core. To build on the host and deploy only the binary, cross-compile with [`cross`](https://github.com/cross-rs/cross):

```sh
cross build --release -p xvc-server-debugbridge --target aarch64-unknown-linux-gnu
```

## Usage

`xvc-bridge` picks a backend automatically, or you can select one explicitly:

```sh
# Auto-detect the backend
xvc-bridge

# Xilinx kernel driver (path is optional; auto-detected if omitted)
xvc-bridge kernel-driver /dev/xilinx_xvc_driver

# UIO device (path is optional; auto-detected if omitted)
xvc-bridge uio-driver /dev/uio0

# Raw memory-mapped bridge at a physical address
xvc-bridge dev-mem-driver 0xA0000000
```

The server binds to `0.0.0.0:2542` by default; override with `--ip` and `--port`.
See `xvc-bridge --help` for all options.

### Automated backend selection

Without an explicit backend, `xvc-bridge` probes for one in this order:

1. **Kernel driver**: used if the `/dev/xilinx_xvc_driver` device node exists.
2. **UIO driver**: used if a [Userspace I/O](https://www.kernel.org/doc/html/v4.18/driver-api/uio-howto.html) device named `debug_bridge` is present.

If neither is found, the server exits; select a backend explicitly as shown above.

## Logging

Diagnostics go through [`env_logger`](https://docs.rs/env_logger/) (default level `info`). Control verbosity with `RUST_LOG`:

```sh
RUST_LOG=debug xvc-bridge uio-driver /dev/uio0
```
