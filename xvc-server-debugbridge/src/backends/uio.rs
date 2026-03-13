//! # UIO Driver Backend
//!
//! For memory-mapped debug bridges that are exposed via the Linux UIO subsystem.
//!
//! ## Example Usage
//!
//! ```ignore
//! use xvc_server_debugbridge::backends::uio::UioDriverBackend;
//! use xvc_server::server::{Server, Config};
//! use std::time::Duration;
//!
//! let driver = UioDriverBackend::new("/dev/uio0", Duration::from_micros(1000))?;
//! let server = Server::new(driver, Config::default());
//! server.listen("127.0.0.1:2542")?;
//! ```
use std::{fs::OpenOptions, io, num::NonZero, path::Path, ptr::NonNull, time::Duration};

use nix::sys::mman::{MapFlags, ProtFlags, mmap, munmap};

use crate::{XvcServer, backends::memory_mapped::{MAP_SIZE, MemoryMappedBackend}};

/// Debug bridge driver based on a Uio device
pub struct UioDriverBackend(MemoryMappedBackend);

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
        Ok(UioDriverBackend(MemoryMappedBackend::new(
            jtag,
            poll_timeout,
        )))
    }
}

impl Drop for UioDriverBackend {
    fn drop(&mut self) {
        if let Some(ptr) = NonNull::new(self.0.mem) {
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

    fn shift(&self, num_bits: u32, tms: &[u8], tdi: &[u8]) -> Box<[u8]> {
        match self.0.shift_data(num_bits, tms, tdi) {
            Ok(result) => result,
            Err(e) => {
                log::error!("UIO shift error: {}", e);
                // The protocol supports no error handling, so we push an empty array.
                Box::default()
            }
        }
    }
}
