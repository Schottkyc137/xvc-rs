//! # Kernel Driver Backend
//!
//! For debug bridges that expose JTAG operations through
//! the official [Linux kernel driver](https://github.com/Xilinx/XilinxVirtualCable/tree/master/jtag/zynqMP/src/driver).
//!
//! ## Example Usage
//!
//! ```ignore
//! use xvc_server_linux::device::DebugBridgeDevice;
//! use xvc_server::server::{Server, Config};
//!
//! let driver = DebugBridgeDevice::new("/dev/xilinx_xvc_driver")?;
//! let server = Server::new(driver, Config::default());
//! server.listen("127.0.0.1:2542".parse()?)?;
//! ```
use nix::{ioctl_read_bad, ioctl_readwrite_bad};
use std::{
    ffi::{CStr, c_char, c_uchar, c_uint, c_ulong},
    fs::{File, OpenOptions},
    io,
    mem::MaybeUninit,
    os::fd::AsRawFd,
    path::Path,
};

use crate::XvcServer;

/// Properties that the user can read from the debug bridge.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct XvcProperties {
    debug_bridge_base_addr: c_ulong,
    debug_bridge_size: c_ulong,
    debug_bridge_compat_string: [c_char; 64],
}

impl XvcProperties {
    /// Returns the base address of the debug bridge as defined in the device-tree
    pub fn debug_bridge_base_address(&self) -> u64 {
        self.debug_bridge_base_addr
    }

    /// Returns the memory size of the debug bridge as defined in the device-tree
    pub fn debug_bridge_size(&self) -> u64 {
        self.debug_bridge_size
    }

    pub fn debug_bridge_compat_string(&self) -> String {
        // SAFETY: The pointer always originates from an ioctl call which respects the conventions.
        let slice = unsafe { CStr::from_ptr(self.debug_bridge_compat_string.as_ptr()) };
        slice
            .to_str()
            .expect("Compatibility string should only be ASCII")
            .to_owned()
    }
}

/// Internal struct used to communicate with the driver
#[repr(C)]
#[derive(Clone, Debug)]
struct XvcIoc {
    opcode: c_uint,
    length: c_uint,
    tms_buf: *const c_uchar,
    tdi_buf: *const c_uchar,
    tdo_buf: *mut c_uchar,
}

const XDMA_RDXVC_PROPS_NR: u32 = 0xD6534402;
const XDMA_IOCXVC_NR: u32 = 0xD6634401;

// Read properties from the device
ioctl_read_bad!(xvc_read_properties, XDMA_RDXVC_PROPS_NR, XvcProperties);
// Perform a shift operation
ioctl_readwrite_bad!(xvc_do_ioc, XDMA_IOCXVC_NR, XvcIoc);

// Note: The following lines cause a mismatch between the ioctl code reported by the kernel
// and the ioctl code reported by userland.
// Therefore, the ioctl request codes are hardcoded above.
//
// Defined in XilinxVirtualCable/jtag/zynqMP/src/driver/xvc_ioctl.h
// const XVC_MAGIC: u32 = 0x58564344;
// ioctl_readwrite!(xvc_do_ioc, XVC_MAGIC, 1, XvcIoc);
// ioctl_read!(xvc_read_properties, XVC_MAGIC, 2, XvcProperties);

/// A device that communicates with a Xilinx Debug Bridge through the dedicated Kernel Driver.
pub struct KernelDriverBackend {
    file: File,
}

impl KernelDriverBackend {
    pub fn new(device_path: impl AsRef<Path>) -> io::Result<KernelDriverBackend> {
        let path = device_path.as_ref();
        log::debug!("Opening kernel driver device: {}", path.display());
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(device_path)?;
        log::debug!("Device file opened successfully");

        // SAFETY: The ioctl call is safe because:
        // - File descriptor is valid (file is open)
        // - properties are structured in a way the driver expects it
        let properties = unsafe {
            let mut props = MaybeUninit::zeroed();
            xvc_read_properties(file.as_raw_fd(), props.as_mut_ptr())?;
            props.assume_init()
        };

        log::info!(
            "Debug bridge properties: base_addr=0x{:x}, size=0x{:x}, compat_string={}",
            properties.debug_bridge_base_address(),
            properties.debug_bridge_size(),
            properties.debug_bridge_compat_string()
        );

        Ok(KernelDriverBackend { file })
    }

    /// Transfers JTAG data.
    /// `num_bits / 8`, rounded up must be the same length as `tms` and `tdi`.
    /// The returned result, if successfull, will also be of that size.
    pub fn shift_data(&self, num_bits: u32, tms: &[u8], tdi: &[u8]) -> io::Result<Box<[u8]>> {
        let num_bytes = num_bits.div_ceil(8) as usize;
        if tms.len() != num_bytes {
            log::error!(
                "TMS buffer size mismatch: expected {}, got {}",
                num_bytes,
                tms.len()
            );
            return Err(io::Error::other("TMS has incorrect size"));
        }
        if tdi.len() != num_bytes {
            log::error!(
                "TDI buffer size mismatch: expected {}, got {}",
                num_bytes,
                tdi.len()
            );
            return Err(io::Error::other("TDI has incorrect size"));
        }

        log::debug!(
            "Kernel driver shift: num_bits={}, num_bytes={}",
            num_bits,
            num_bytes
        );
        log::trace!("Kernel driver shift TMS: {:02x?}", tms);
        log::trace!("Kernel driver shift TDI: {:02x?}", tdi);

        let mut result = vec![0; num_bytes].into_boxed_slice();
        let mut xvc_ioc = XvcIoc {
            opcode: 1,
            length: num_bits as c_uint,
            tms_buf: tms.as_ptr(),
            tdi_buf: tdi.as_ptr(),
            tdo_buf: result.as_mut_ptr(),
        };
        // SAFETY: The ioctl call is safe because:
        // - File descriptor is valid (self.file is open)
        // - Buffers are valid for the duration of the call
        // - Buffer sizes match the num_bits parameter
        unsafe {
            xvc_do_ioc(self.file.as_raw_fd(), &mut xvc_ioc)?;
        }

        log::trace!("Kernel driver shift result TDO: {:02x?}", &result[..]);
        Ok(result)
    }
}

impl XvcServer for KernelDriverBackend {
    fn set_tck(&self, period_ns: u32) -> u32 {
        log::debug!("Kernel driver set_tck: period_ns={}", period_ns);
        period_ns
    }

    fn shift(&self, num_bits: u32, tms: Box<[u8]>, tdi: Box<[u8]>) -> Box<[u8]> {
        match self.shift_data(num_bits, &tms, &tdi) {
            Ok(result) => result,
            Err(e) => {
                log::error!("Kernel driver shift error: {}", e);
                // The server supports no error handling, so we push an empty array.
                Box::default()
            }
        }
    }
}
