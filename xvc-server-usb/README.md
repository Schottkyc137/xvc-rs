# xvc-server-usb

[Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) server that talks to a target through a USB-to-JTAG bridge.
This crate provides the `xvc-usb` binary, which exposes the JTAG chain over TCP so tools like Vivado can reach a board connected over USB.

This crate is part of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) project.

## Supported hardware

Supported chips are from the FTDI family (currently: FT2232H, FT4232H, FT232H).
These are the chips found on most AMD/Xilinx and Digilent evaluation boards.

## Requirements

- A supported FTDI adapter connected over USB.
- Permission to access the USB device (see [Permissions](#permissions)).

libusb is built from source (via the `rusb` `vendored` feature), so no system libusb is required to build.

## Installation

```sh
cargo install xvc-server-usb
```

This installs the `xvc-usb` binary. If the host is an ARM board such as a Raspberry Pi, cross-compile for it with [`cross`](https://github.com/cross-rs/cross):

```sh
cross build --release -p xvc-server-usb --target aarch64-unknown-linux-gnu
```

<!-- TODO: confirm which cross-compilation targets are tested. -->

## Usage

```sh
# Serve the first FTDI device found
xvc-usb

# Select a specific FTDI channel (e.g. on multi-interface FT2232H/FT4232H parts)
xvc-usb --ftdi-port 1
```

The server binds to `0.0.0.0:2542` by default; override with `--ip` and `--port`.
When several matching devices are connected, `xvc-usb` prompts you to choose one.
Pass `--non-interactive` to fail instead of prompting.
The `--loopback` flag runs the FTDI chip in loopback for testing without a target.
See `xvc-usb --help` for all options.

## Permissions

Accessing an FTDI device through libusb might needs extra steps.
Refer to the [libusb FAQ](https://github.com/libusb/libusb/wiki/FAQ) for more.

## Logging

Diagnostics go through [`env_logger`](https://docs.rs/env_logger/) (default level `info`).
Control verbosity with `RUST_LOG`:

```sh
RUST_LOG=debug xvc-usb
```
