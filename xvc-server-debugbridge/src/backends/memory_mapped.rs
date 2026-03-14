use std::{
    io,
    ptr::{read_volatile, write_volatile},
    time::{Duration, Instant},
};

pub(super) const MAP_SIZE: usize = 0x10000;

// Word (u32) offsets into the memory-mapped register block
const LENGTH_OFFSET: usize = 0;
const TMS_REG_OFFSET: usize = 1;
const TDI_REG_OFFSET: usize = 2;
const TDO_REG_OFFSET: usize = 3;
const CONTROL_REG_OFFSET: usize = 4;

/// A backend that uses the memory-mapped AXI to JTAG bridge.
/// Used by the UIO and the DevMem Backend.
pub struct MemoryMappedBackend {
    pub mem: *mut u32,
    /// The driver must poll the Debug Bridge since there are no interrupt lines.
    /// This timeout defines how long a poll may take before issuing a timeout error.
    pub poll_timeout: Duration,
}

// SAFETY: `mem` points to a memory-mapped hardware register block that is
// stable for the lifetime of the backend.
unsafe impl Send for MemoryMappedBackend {}

fn u32_from_u8_slice(slice: &[u8]) -> u32 {
    assert!(slice.len() <= 4);
    let mut buf = [0u8; 4];
    buf[..slice.len()].copy_from_slice(slice);
    u32::from_ne_bytes(buf)
}

impl MemoryMappedBackend {
    pub fn new(mem: *mut u32, poll_timeout: Duration) -> MemoryMappedBackend {
        MemoryMappedBackend { mem, poll_timeout }
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
                write_volatile(self.mem.add(LENGTH_OFFSET), shift_num_bits);
                write_volatile(
                    self.mem.add(TMS_REG_OFFSET),
                    u32_from_u8_slice(&tms[..shift_num_bytes as usize]),
                );
                write_volatile(
                    self.mem.add(TDI_REG_OFFSET),
                    u32_from_u8_slice(&tdi[..shift_num_bytes as usize]),
                );
                write_volatile(self.mem.add(CONTROL_REG_OFFSET), 0x01);

                let poll_until_ready = || {
                    let start = Instant::now();
                    while start.elapsed() < self.poll_timeout {
                        if read_volatile(self.mem.add(CONTROL_REG_OFFSET)) == 0 {
                            return Ok(());
                        }
                    }
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Timed out while waiting for JTAG response",
                    ))
                };
                poll_until_ready()?;

                &read_volatile(self.mem.add(TDO_REG_OFFSET)).to_ne_bytes()
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
