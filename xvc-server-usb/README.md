# xvc-server-usb

[Xilinx Virtual Cable (XVC) 1.0](https://github.com/Xilinx/XilinxVirtualCable) server that talks to a target through an FTDI USB-to-JTAG bridge.
It installs the `xvc-usb` binary, which exposes the JTAG chain over TCP so tools like Vivado can reach a board connected over USB.

This crate is part of the [`xvc-rs`](https://github.com/Schottkyc137/xvc-rs) workspace and builds on [`xvc-server`](https://crates.io/crates/xvc-server).

## Supported hardware

FTDI FT232H / FT2232H / FT4232H USB-to-JTAG bridges — the chips found on most AMD/Xilinx and Digilent evaluation boards.

<!-- TODO: confirm the exact list of tested chips and boards. -->

## Requirements

- A supported FTDI adapter connected over USB.
- Permission to access the USB device (see [Permissions](#permissions)).

libusb is built from source (via the `rusb` `vendored` feature), so no system libusb is required to build.

<!-- TODO: confirm which operating systems are tested (Linux / macOS / Windows). -->

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
When several matching devices are connected, `xvc-usb` prompts you to choose one — pass `--non-interactive` to fail instead of prompting.
The `--loopback` flag runs the FTDI chip in loopback for testing without a target.
See `xvc-usb --help` for all options.

## Permissions

Accessing an FTDI device through libusb usually needs extra setup:

- **Linux:** **TODO** — udev rule granting access to the FTDI VID/PID, and note
  whether the `ftdi_sio` / `usbserial` kernel driver must be unbound first.
- **macOS:** **TODO** — whether Apple's built-in FTDI driver must be unloaded.
- **Windows:** **TODO** — WinUSB driver installation (e.g. via
  [Zadig](https://zadig.akeo.ie/)).

## Logging

Diagnostics go through [`env_logger`](https://docs.rs/env_logger/) (default level `info`).
Control verbosity with `RUST_LOG`:

```sh
RUST_LOG=debug xvc-usb
```
