# xvc-server-debugbridge

Ready-to-run [Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) server for [AMD Debug Bridges](https://www.amd.com/en/products/adaptive-socs-and-fpgas/intellectual-property/debug-bridge.html) on Linux.
It installs the `xvc-bridge` binary, which exposes a Debug Bridge instantiated in your FPGA design over TCP so tools like Vivado can reach it.

This crate is part of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) workspace and builds on [`xvc-server`](https://crates.io/crates/xvc-server).

## Requirements

- **Linux** — the backends use `/dev` device nodes and Linux syscalls (ioctl, UIO, mmap).
- A [Debug Bridge](https://docs.amd.com/v/u/en-US/pg245-debug-bridge) (or an equivalent) instantiated on the target FPGA.
- Permission to access the chosen device node — typically **root**:
  `/dev/xilinx_xvc_driver`, `/dev/uioN`, or `/dev/mem`.

## Installation

```sh
cargo install xvc-server-debugbridge
```

This installs the `xvc-bridge` binary. The server usually runs on the SoC's ARM core, so cross-compile for it with [`cross`](https://github.com/cross-rs/cross):

```sh
cross build --release -p xvc-server-debugbridge --target aarch64-unknown-linux-gnu
```

## Usage

`xvc-bridge` picks a backend automatically, or you can select one explicitly:

```sh
# Auto-detect the backend (kernel driver, then UIO)
xvc-bridge

# Xilinx kernel driver (path is optional; auto-detected if omitted)
xvc-bridge kernel-driver /dev/xilinx_xvc_driver

# UIO device
xvc-bridge uio-driver /dev/uio0

# Raw /dev/mem at a physical address
xvc-bridge dev-mem-driver 0xA0000000
```

The server binds to `0.0.0.0:2542` by default; override with `--ip` and `--port`.
See `xvc-bridge --help` for all options.

## Logging

Diagnostics go through [`env_logger`](https://docs.rs/env_logger/) (default level `info`). Control verbosity with `RUST_LOG`:

```sh
RUST_LOG=debug xvc-bridge uio-driver /dev/uio0
```
