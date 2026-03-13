//! # DevMem Driver Backend
//!
//! Avoids the kernel driver entirely and performs all steps from user-space
//!
//! ## Example Usage
//!
//! ```ignore
//! use xvc_server_debugbridge::backends::devmem::DevMemBackend;
//! use xvc_server::server::{Server, Config};
//! use std::time::Duration;
//!
//! let driver = DevMemBackend::new(0xFF00_0000, Duration::from_micros(1000))?;
//! let server = Server::new(driver, Config::default());
//! server.listen("127.0.0.1:2542")?;
//! ```

use std::{fs::OpenOptions, io, num::NonZero, path::Path, ptr::NonNull, time::Duration};

use nix::sys::mman::{MapFlags, ProtFlags, mmap, munmap};
use xvc_server::XvcServer;

use crate::backends::memory_mapped::{MAP_SIZE, MemoryMappedBackend};

/// Debug bridge driver based on a Uio device
pub struct DevMemBackend(MemoryMappedBackend);

impl DevMemBackend {
    pub fn new(address: i64, poll_timeout: Duration) -> io::Result<DevMemBackend> {
        Self::new_with_path("/dev/mem", address, poll_timeout)
    }

    pub fn new_with_path(
        path: impl AsRef<Path>,
        address: i64,
        poll_timeout: Duration,
    ) -> io::Result<DevMemBackend> {
        let device_path = path.as_ref();
        log::debug!("Opening DevMem device: {}", device_path.display());
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(device_path)?;
        log::debug!("DevMem file opened successfully");

        let mem = unsafe {
            log::debug!(
                "Mapping DevMem (address=0x{:x}; size=0x{:x})",
                address,
                MAP_SIZE
            );
            let ptr = mmap(
                None,
                NonZero::new(MAP_SIZE).unwrap(),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                file,
                address,
            )?;
            log::info!("DevMem memory mapped successfully");
            ptr.as_ptr() as *mut u32
        };
        Ok(DevMemBackend(MemoryMappedBackend::new(mem, poll_timeout)))
    }
}

impl Drop for DevMemBackend {
    fn drop(&mut self) {
        if let Some(ptr) = NonNull::new(self.0.mem) {
            unsafe {
                let _ = munmap(ptr.cast(), MAP_SIZE);
            }
        }
    }
}

impl XvcServer for DevMemBackend {
    fn set_tck(&self, period_ns: u32) -> u32 {
        log::debug!("DevMem set_tck: period_ns={}", period_ns);
        period_ns
    }

    fn shift(&self, num_bits: u32, tms: Box<[u8]>, tdi: Box<[u8]>) -> Box<[u8]> {
        match self.0.shift_data(num_bits, &tms, &tdi) {
            Ok(result) => result,
            Err(e) => {
                log::error!("DevMem shift error: {}", e);
                // The protocol supports no error handling, so we push an empty array.
                Box::default()
            }
        }
    }
}
