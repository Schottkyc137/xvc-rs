# XVC Server for the AMD Debug Bridge

Linux-specific backend implementations of the XVC (Xilinx Virtual Cable) server for [AMD Debug Bridges](https://www.amd.com/en/products/adaptive-socs-and-fpgas/intellectual-property/debug-bridge.html).

## Features

- **Command-line Binary**: Ready-to-use server executable
- **Multiple Backends**:
  - **Ioctl Driver**: Kernel driver communication via ioctl syscalls
  - **UIO Driver**: Userspace I/O for memory-mapped FPGA interfaces

## Usage

This crate provides a command-line server binary:

```bash
# Automatically select the right driver
xvc-bridge

# Start using the kernel driver
xvc-bridge kernel-driver /dev/xilinx_xvc_driver

# Start using the UIO driver
xvc-bridge uio-driver /dev/uio0
```

See `xvc-bridge --help` for all available options.

## Environment Variables

- `RUST_LOG`: configure log levels (e.g., `RUST_LOG=debug`)

### Example:

```bash
RUST_LOG=debug xvc-bridge --ip 192.168.99.217 uio-driver /dev/uio0
```

## See Also

- [xvc-server](../xvc-server/) - Core protocol implementation
- [xvc-protocol](../xvc-protocol/) - Protocol encoding/decoding
- [Xilinx Virtual Cable](https://github.com/Xilinx/XilinxVirtualCable) - Official XVC specification
- [Debug Bridge in Vivado](https://docs.amd.com/r/en-US/ug908-vivado-programming-debugging/Debug-Bridge) - Debug Bridge documentation
