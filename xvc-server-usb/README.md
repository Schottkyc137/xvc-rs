# XVC Server for USB-to-JTAG chips

Platform-independent XVC (Xilinx Virtual Cable) server implementation for connecting to an AMD device via a USB-to-JTAG FTDI bridge.

Currently supported are the FTDI USB to JTAG adaptor families (FT232H/FT2232H/FT4232H).
These are the most common chips on AMD/Xilinx and Digilent eval boards

## Usage

This crate provides a command-line server binary. See `xvc-usb --help` for available options.
