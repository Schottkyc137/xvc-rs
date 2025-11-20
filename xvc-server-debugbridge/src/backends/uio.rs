//! # UIO Driver Backend
//!
//! For memory-mapped debug bridges that are exposed via the Linux UIO subsystem.
//!
//! ## Example Usage
//!
//! ```ignore
//! use xvc_server_linux::device::UioDebugBridgeDriver;
//! use xvc_server::server::{Server, Config};
//!
//! let driver = UioDebugBridgeDriver::new("/dev/uio0")?;
//! let server = Server::new(driver, Config::default());
//! server.listen("127.0.0.1:2542")?;
//! ```
use std::{
    fs::OpenOptions,
    io,
    num::NonZero,
    path::Path,
    ptr::{NonNull, read_volatile, write_volatile},
    time::{Duration, Instant},
};

use nix::sys::mman::{MapFlags, ProtFlags, mmap, munmap};

use crate::XvcServer;

const LENGTH_OFFSET: usize = 0;
const TMS_REG_OFFSET: usize = 4;
const TDI_REG_OFFSET: usize = 8;
const TDO_REG_OFFSET: usize = 12;
const CONTROL_REG_OFFSET: usize = 16;

const MAP_SIZE: usize = 0x10000;

/// Debug bridge driver based on a Uio device
pub struct UioDriverBackend {
    jtag: *mut u32,
    /// The driver must poll the Debug Bridge since there are no interrupt lines.
    /// This timeout defines how long a poll may take before issuing a timeout error.
    poll_timeout: Duration,
}

fn u32_from_u8_slice(slice: &[u8]) -> u32 {
    assert!(slice.len() <= 4);
    let mut buf = [0u8; 4];
    buf[..slice.len()].copy_from_slice(slice);
    u32::from_ne_bytes(buf)
}

impl UioDriverBackend {
    pub fn new(path: impl AsRef<Path>, poll_timeout: Duration) -> io::Result<UioDriverBackend> {
        let device_path = path.as_ref();
        log::debug!("Opening UIO device: {}", device_path.display());
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        log::debug!("UIO device file opened successfully");

        let jtag = unsafe {
            log::debug!("Mapping UIO memory (size=0x{:x})", MAP_SIZE);
            let ptr = mmap(
                None,
                NonZero::new(MAP_SIZE).unwrap(),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                file,
                0,
            )?;
            log::info!("UIO memory mapped successfully");
            ptr.as_ptr() as *mut u32
        };
        Ok(UioDriverBackend { jtag, poll_timeout })
    }

    // Note this is an adapted version of the Xilinx driver
    pub fn shift_data(
        &self,
        num_bits: u32,
        mut tms: &[u8],
        mut tdi: &[u8],
    ) -> io::Result<Box<[u8]>> {
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

        log::debug!("UIO shift: num_bits={}, num_bytes={}", num_bits, num_bytes);
        log::trace!("UIO shift TMS: {:02x?}", tms);
        log::trace!("UIO shift TDI: {:02x?}", tdi);

        let mut result = Vec::with_capacity(num_bytes);
        let mut bits_left = num_bits;
        let mut iteration = 0u32;

        while !tms.is_empty() {
            let shift_num_bits = if tms.len() <= 4 { bits_left } else { 32 };
            let shift_num_bytes = shift_num_bits.div_ceil(8);

            log::trace!(
                "UIO shift iteration {}: bytes_left={}, bits_left={}, shift_num_bits={}",
                iteration,
                tms.len(),
                bits_left,
                shift_num_bits
            );

            let tdo = unsafe {
                write_volatile(self.jtag.add(LENGTH_OFFSET / 4), shift_num_bits);
                write_volatile(
                    self.jtag.add(TMS_REG_OFFSET / 4),
                    u32_from_u8_slice(&tms[..shift_num_bytes as usize]),
                );
                write_volatile(
                    self.jtag.add(TDI_REG_OFFSET / 4),
                    u32_from_u8_slice(&tdi[..shift_num_bytes as usize]),
                );
                write_volatile(self.jtag.add(CONTROL_REG_OFFSET / 4), 0x01);

                let poll_until_ready = || {
                    let start = Instant::now();
                    while start.elapsed() < self.poll_timeout {
                        if read_volatile(self.jtag.add(CONTROL_REG_OFFSET / 4)) == 0 {
                            return Ok(());
                        }
                    }
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Timed out while waiting for JTAG response",
                    ))
                };
                poll_until_ready()?;

                &read_volatile(self.jtag.add(TDO_REG_OFFSET / 4)).to_ne_bytes()
                    [..shift_num_bytes as usize]
            };

            log::trace!(
                "UIO shift iteration {} result: tdo: {:02x?}",
                iteration,
                tdo
            );

            result.extend_from_slice(tdo);

            tms = &tms[shift_num_bytes as usize..];
            tdi = &tdi[shift_num_bytes as usize..];

            bits_left -= shift_num_bits;
            iteration += 1;
        }

        log::trace!("UIO shift result TDO: {:02x?}", &result[..]);
        Ok(result.into_boxed_slice())
    }
}

impl Drop for UioDriverBackend {
    fn drop(&mut self) {
        if let Some(ptr) = NonNull::new(self.jtag) {
            unsafe {
                let _ = munmap(ptr.cast(), MAP_SIZE);
            }
        }
    }
}

impl XvcServer for UioDriverBackend {
    fn set_tck(&self, period_ns: u32) -> u32 {
        log::debug!("UIO set_tck: period_ns={}", period_ns);
        period_ns
    }

    fn shift(&self, num_bits: u32, tms: Box<[u8]>, tdi: Box<[u8]>) -> Box<[u8]> {
        match self.shift_data(num_bits, &tms, &tdi) {
            Ok(result) => result,
            Err(e) => {
                log::error!("UIO shift error: {}", e);
                // The protocol supports no error handling, so we push an empty array.
                Box::default()
            }
        }
    }
}
