use ftdi_mpsse::MpsseCmdBuilder;
use xvc_server::XvcServer;

use crate::ftdi_device::FtdiJtagDevice;

pub struct FtdiServer {
    device: FtdiJtagDevice,
}

impl FtdiServer {
    pub fn new(device: FtdiJtagDevice) -> FtdiServer {
        FtdiServer { device }
    }
}

const FTDI_CLOCK_RATE: u32 = 60000000;

fn count_for_frequency(frequency: u32) -> (u32, u32) {
    let frequency = frequency.max(1);
    let count = (FTDI_CLOCK_RATE / 2).div_ceil(frequency).clamp(1, 0x10000);
    let actual = FTDI_CLOCK_RATE / (2 * count);
    let r = frequency as f64 / actual as f64;
    if !(0.999..=1.001).contains(&r) {
        log::warn!("{frequency} Hz requested, {actual} Hz actual");
    }
    (count - 1, actual) // return the count register directly
}

impl FtdiServer {
    fn set_clock_speed(&self, frequency: u32) -> rusb::Result<u32> {
        let (count, actual) = count_for_frequency(frequency);
        let cmd = MpsseCmdBuilder::new().set_clock(count, Some(false));
        self.device.write(cmd.as_slice())?;
        Ok(actual)
    }
}

impl XvcServer for FtdiServer {
    type Err = rusb::Error;

    fn set_tck(&self, period_ns: u32) -> Result<u32, Self::Err> {
        if period_ns == 0 {
            log::error!("set tck to zero");
            return Ok(period_ns);
        }
        let freq = 1_000_000_000 / period_ns;
        self.set_clock_speed(freq).map(|f| 1_000_000_000 / f)
    }

    fn shift(
        &self,
        num_bits: u32,
        tms: &[u8],
        tdi: &[u8],
        tdo: &mut [u8],
    ) -> Result<(), Self::Err> {
        self.device.shift_chunks(num_bits, tdi, tms, tdo)
    }
}
