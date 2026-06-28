use std::time::Duration;

use ftdi_mpsse::{ClockBits, ClockData, ClockTMS, MpsseCmdBuilder, mpsse};
use rusb::{Context, DeviceHandle, UsbContext};

const FTDI_PIN_TCK: u8 = 0x1;
const FTDI_PIN_TDI: u8 = 0x2;
#[allow(unused)]
const FTDI_PIN_TDO: u8 = 0x4;
const FTDI_PIN_TMS: u8 = 0x8;

const BMREQTYPE_OUT: u8 = rusb::constants::LIBUSB_REQUEST_TYPE_VENDOR
    | rusb::constants::LIBUSB_RECIPIENT_DEVICE
    | rusb::constants::LIBUSB_ENDPOINT_OUT;

const BREQ_RESET: u8 = 0x0;
const BREQ_SET_LATENCY: u8 = 0x09;
const BREQ_SET_BITMODE: u8 = 0x0B;

const WVAL_RESET_RESET: u16 = 0x0;
const WVAL_SET_BITMODE_MPSSE: u16 = 0x0200 | (FTDI_PIN_TCK | FTDI_PIN_TDI | FTDI_PIN_TMS) as u16;
const WVAL_RESET_PURGE_TX: u16 = 0x02;
const WVAL_RESET_PURGE_RX: u16 = 0x01;

mpsse! {
    const INIT_CMD = {
        disable_loopback();
        disable_3phase_data_clocking();
        disable_adaptive_data_clocking();
        set_gpio_lower(FTDI_PIN_TMS, FTDI_PIN_TCK | FTDI_PIN_TDI | FTDI_PIN_TMS);
    };

    const INIT_CMD_LOOPBACK = {
        enable_loopback();
        disable_3phase_data_clocking();
        disable_adaptive_data_clocking();
        set_gpio_lower(FTDI_PIN_TMS, FTDI_PIN_TCK | FTDI_PIN_TDI | FTDI_PIN_TMS);
    };
}

pub struct FtdiJtagDevice<H: UsbHandle = DeviceHandle<Context>> {
    handle: H,
    iface: u8,
    bulk_out_ep_adr: u8,
    bulk_out_request_size: u16,
    bulk_in_ep_adr: u8,
    bulk_in_request_size: u16,
    timeout: Duration,
}

pub trait UsbHandle {
    fn write(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> rusb::Result<usize>;

    fn read(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> rusb::Result<usize>;
}

impl<C: UsbContext> UsbHandle for DeviceHandle<C> {
    fn write(&self, endpoint: u8, buf: &[u8], timeout: Duration) -> rusb::Result<usize> {
        self.write_bulk(endpoint, buf, timeout)
    }

    fn read(&self, endpoint: u8, buf: &mut [u8], timeout: Duration) -> rusb::Result<usize> {
        self.read_bulk(endpoint, buf, timeout)
    }
}

impl<C: UsbContext> FtdiJtagDevice<DeviceHandle<C>> {
    pub fn claim_interface(&self) -> rusb::Result<()> {
        match self.handle.set_auto_detach_kernel_driver(true) {
            Ok(()) | Err(rusb::Error::NotSupported) => {}
            Err(other) => return Err(other),
        }
        self.handle.claim_interface(self.iface)?;

        Ok(())
    }

    pub fn write_control(&self, request_type: u8, request: u8, value: u16) -> rusb::Result<usize> {
        self.handle.write_control(
            request_type,
            request,
            value,
            (self.iface as u16) + 1,
            &[],
            self.timeout,
        )
    }

    pub fn reset(&self) -> rusb::Result<usize> {
        self.write_control(BMREQTYPE_OUT, BREQ_RESET, WVAL_RESET_RESET)
    }

    pub fn set_bitmode_mpsse(&self) -> rusb::Result<usize> {
        self.write_control(BMREQTYPE_OUT, BREQ_SET_BITMODE, WVAL_SET_BITMODE_MPSSE)
    }

    pub fn set_latency(&self, latency: u16) -> rusb::Result<usize> {
        self.write_control(BMREQTYPE_OUT, BREQ_SET_LATENCY, latency)
    }

    pub fn purge_tx(&self) -> rusb::Result<usize> {
        self.write_control(BMREQTYPE_OUT, BREQ_RESET, WVAL_RESET_PURGE_TX)
    }

    pub fn purge_rx(&self) -> rusb::Result<usize> {
        self.write_control(BMREQTYPE_OUT, BREQ_RESET, WVAL_RESET_PURGE_RX)
    }

    pub fn ftdi_init(&self, loopback: bool) -> rusb::Result<()> {
        self.reset()?;
        self.set_bitmode_mpsse()?;
        self.set_latency(2)?;
        self.purge_tx()?;
        self.purge_rx()?;

        if loopback {
            self.write(&INIT_CMD_LOOPBACK)?;
        } else {
            self.write(&INIT_CMD)?;
        }

        Ok(())
    }
}

impl<H: UsbHandle> FtdiJtagDevice<H> {
    pub fn new(
        handle: H,
        iface: u8,
        bulk_out_ep_adr: u8,
        bulk_out_request_size: u16,
        bulk_in_ep_adr: u8,
        bulk_in_request_size: u16,
        timeout: Duration,
    ) -> FtdiJtagDevice<H> {
        FtdiJtagDevice {
            handle,
            iface,
            bulk_out_ep_adr,
            bulk_out_request_size,
            bulk_in_ep_adr,
            bulk_in_request_size,
            timeout,
        }
    }

    pub fn write(&self, mut values: &[u8]) -> rusb::Result<()> {
        while !values.is_empty() {
            let written = self
                .handle
                .write(self.bulk_out_ep_adr, values, self.timeout)?;
            values = &values[written..];
        }
        Ok(())
    }

    pub fn read(&self, out: &mut [u8]) -> rusb::Result<()> {
        let packet = self.bulk_in_request_size as usize;
        let mut buf = vec![0u8; packet];
        let mut filled = 0;

        while filled < out.len() {
            let n = self
                .handle
                .read(self.bulk_in_ep_adr, &mut buf, self.timeout)?;
            let mut off = 0;
            while off < n {
                let end = (off + packet).min(n);
                if end > off + 2 {
                    let payload = &buf[off + 2..end]; // strip 2 status bytes
                    let take = payload.len().min(out.len() - filled);
                    out[filled..filled + take].copy_from_slice(&payload[..take]);
                    filled += take;
                }
                off = end;
            }
        }
        Ok(())
    }

    pub fn shift_chunks(
        &self,
        mut n_bits: u32,
        tdi: &[u8],
        tms: &[u8],
        tdo: &mut [u8],
    ) -> rusb::Result<()> {
        let mut i_bit: i32 = 0x01;
        let mut i_index: usize = 0;
        let mut tdo_bit: i32 = 0x01;
        let mut tdo_index: usize = 0;
        let mut rx_bitcounts = vec![0u16; self.bulk_in_request_size as usize];

        while n_bits != 0 {
            // Reset the command buffer for this chunk (C: usb->txCount = 0).
            let mut builder = MpsseCmdBuilder::new();
            let mut rx_bytes_wanted: i32 = 0;
            let mut rx_bitcount_index: usize = 0;

            // do { ... } while (...)
            loop {
                /*
                 * Stash TMS bits until bit limit reached or TDI would change state
                 */
                let tdi_first_state: bool = (tdi[i_index] as i32 & i_bit) != 0;
                let mut cmd_bitcount: u8 = 0;
                let mut cmd_bit: u8 = 0x01;
                let mut tms_bits: u8 = 0;
                let mut tms_bit: u8;
                // do { ... } while (...)
                loop {
                    tms_bit = if (tms[i_index] as i32 & i_bit) != 0 {
                        cmd_bit
                    } else {
                        0
                    };
                    tms_bits |= tms_bit;
                    if i_bit == 0x80 {
                        i_bit = 0x01;
                        i_index += 1;
                    } else {
                        i_bit <<= 1;
                    }
                    cmd_bitcount += 1;
                    cmd_bit <<= 1;
                    if !((cmd_bitcount < 6)
                        && ((cmd_bitcount as u32) < n_bits)
                        && (((tdi[i_index] as i32 & i_bit) != 0) == tdi_first_state))
                    {
                        break;
                    }
                }

                /*
                 * Duplicate the final TMS bit so the TMS pin holds
                 * its value for subsequent TDI shift commands.
                 * This is why the bit limit above is 6 and not 7 since
                 * we need space to hold the copy of the final bit.
                 */
                tms_bits |= tms_bit << 1;
                let tms_state: bool = tms_bit != 0;

                /*
                 * Send the TMS bits and TDI value.
                 */
                builder = builder.clock_tms(
                    ClockTMS::NegTMSPosTDO,
                    tms_bits,
                    tdi_first_state,
                    cmd_bitcount,
                );
                rx_bitcounts[rx_bitcount_index] = cmd_bitcount as u16;
                rx_bitcount_index += 1;
                rx_bytes_wanted += 1;
                n_bits -= cmd_bitcount as u32;

                /*
                 * Stash TDI bits until bit limit reached
                 * or TMS change of state
                 * or transmitter buffer capacity reached.
                 */
                cmd_bitcount = 0;
                cmd_bit = 0x01;
                let mut cmd_index: usize = 0;
                let mut buf = vec![0u8; self.bulk_out_request_size as usize];
                buf[0] = 0;
                while (n_bits != 0)
                    && (((tms[i_index] as i32 & i_bit) != 0) == tms_state)
                    && (((builder.as_slice().len() + (cmd_bitcount as usize / 8)) as u16)
                        < (self.bulk_out_request_size - 5))
                {
                    if (tdi[i_index] as i32 & i_bit) != 0 {
                        buf[cmd_index] |= cmd_bit;
                    }
                    if cmd_bit == 0x80 {
                        cmd_bit = 0x01;
                        cmd_index += 1;
                        buf[cmd_index] = 0;
                    } else {
                        cmd_bit <<= 1;
                    }
                    if i_bit == 0x80 {
                        i_bit = 0x01;
                        i_index += 1;
                    } else {
                        i_bit <<= 1;
                    }
                    cmd_bitcount += 1;
                    n_bits -= 1;
                }

                /*
                 * Send stashed TDI bits
                 */
                if cmd_bitcount > 0 {
                    let cmd_bytes = cmd_bitcount / 8;
                    rx_bitcounts[rx_bitcount_index] = cmd_bitcount as u16;
                    rx_bitcount_index += 1;
                    if cmd_bitcount >= 8 {
                        rx_bytes_wanted += cmd_bytes as i32;
                        cmd_bitcount -= cmd_bytes * 8;
                        builder =
                            builder.clock_data(ClockData::LsbPosIn, &buf[..cmd_bytes as usize]);
                    }
                    if cmd_bitcount != 0 {
                        rx_bytes_wanted += 1;
                        builder = builder.clock_bits(
                            ClockBits::LsbPosIn,
                            buf[cmd_bytes as usize],
                            cmd_bitcount,
                        );
                    }
                }

                if !((n_bits != 0)
                    && (((builder.as_slice().len() + (cmd_bitcount as usize / 8)) as u16)
                        < (self.bulk_out_request_size - 6)))
                {
                    break;
                }
            }

            /*
             * Shift
             */
            self.write(builder.as_slice())?;
            let mut rx_buf = vec![0u8; rx_bytes_wanted as usize];
            self.read(&mut rx_buf)?;

            /*
             * Process received data
             */
            let mut rx_index: usize = 0;
            for i in 0..rx_bitcount_index {
                let mut rx_bitcount: i32 = rx_bitcounts[i] as i32;
                let mut rx_bit: i32 = if rx_bitcount < 8 {
                    0x1 << (8 - rx_bitcount)
                } else {
                    0x01
                };
                while rx_bitcount != 0 {
                    rx_bitcount -= 1;
                    if tdo_bit == 0x1 {
                        tdo[tdo_index] = 0;
                    }
                    if (rx_buf[rx_index] as i32 & rx_bit) != 0 {
                        tdo[tdo_index] |= tdo_bit as u8;
                    }
                    if rx_bit == 0x80 {
                        rx_bit = if rx_bitcount < 8 {
                            0x1 << (8 - rx_bitcount)
                        } else {
                            0x01
                        };
                        rx_index += 1;
                    } else {
                        rx_bit <<= 1;
                    }
                    if tdo_bit == 0x80 {
                        tdo_bit = 0x01;
                        tdo_index += 1;
                    } else {
                        tdo_bit <<= 1;
                    }
                }
            }
            if rx_index as i32 != rx_bytes_wanted {
                log::warn!("consumed {} but supplied {}", rx_index, rx_bytes_wanted);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::{cell::RefCell, time::Duration};

    use crate::ftdi_device::{FtdiJtagDevice, UsbHandle};

    // Simple recorder that just records the chunks that were sent
    struct Recorder {
        pub received: RefCell<Vec<Vec<u8>>>,
    }

    impl Recorder {
        pub fn new() -> Recorder {
            Recorder {
                received: RefCell::default(),
            }
        }
    }

    impl UsbHandle for Recorder {
        fn write(
            &self,
            _endpoint: u8,
            buf: &[u8],
            _timeout: std::time::Duration,
        ) -> rusb::Result<usize> {
            self.received.borrow_mut().push(buf.to_owned());
            Ok(buf.len())
        }

        fn read(
            &self,
            _endpoint: u8,
            buf: &mut [u8],
            _timeout: std::time::Duration,
        ) -> rusb::Result<usize> {
            Ok(buf.len())
        }
    }

    fn make_dev(out_size: u16, in_size: u16) -> FtdiJtagDevice<Recorder> {
        FtdiJtagDevice {
            handle: Recorder::new(),
            iface: 0,
            bulk_out_ep_adr: 0x02,
            bulk_out_request_size: out_size,
            bulk_in_ep_adr: 0x81,
            bulk_in_request_size: in_size,
            timeout: Duration::from_secs(1),
        }
    }

    // Golden command-stream tests. Expected bytes are hand-traced from the
    // algorithm and the ftdi-mpsse encoding:
    //   clock_tms  => [0x6B, len-1, data | (tdi << 7)]
    //   clock_bits => [0x3B, len-1, data]
    //   clock_data => [0x39, lo(len-1), hi(len-1), bytes..]
    // They should match the BerkeleyLab C reference byte-for-byte. The
    // Recorder's read returns zeros, so TDO is not asserted here.

    #[test]
    fn one_tms_bit() {
        // 1 bit, TMS low, TDI high -> a single 1-bit clock_tms carrying TDI in
        // bit 7 of the data byte.
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 1];
        dev.shift_chunks(1, &[0x01], &[0x00], &mut tdo).unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent.len(), 1, "expected a single chunk");
        assert_eq!(sent[0], [0x6B, 0x00, 0x80]);
    }

    #[test]
    fn three_tms_bits() {
        // 3 bits, all TMS (TDI constant) -> one 3-bit clock_tms. TMS 0b101 plus
        // the duplicated final bit gives data 0x0D.
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 1];
        dev.shift_chunks(3, &[0x00], &[0x05], &mut tdo).unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent[0], [0x6B, 0x02, 0x0D]);
    }

    #[test]
    fn tms_then_tdi_bits() {
        // 4 bits, TMS low: the leading bit goes out as a 1-bit clock_tms, the
        // remaining 3 TDI bits as a 3-bit clock_bits.
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 1];
        dev.shift_chunks(4, &[0x0A], &[0x00], &mut tdo).unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent[0], [0x6B, 0x00, 0x00, 0x3B, 0x02, 0x05]);
    }

    #[test]
    fn tms_then_tdi_byte() {
        // 9 bits, TMS low, TDI bit1 != bit0 so the leading clock_tms is 1 bit;
        // the next 8 TDI bits (all 1) pack into a single-byte clock_data.
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 2];
        dev.shift_chunks(9, &[0xFE, 0x01], &[0x00, 0x00], &mut tdo)
            .unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent[0], [0x6B, 0x00, 0x00, 0x39, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn large_shift_splits_into_independent_chunks() {
        // Regression test for the per-chunk buffer reset. With a tiny OUT
        // request size, 200 bits span several USB transfers. Each recorded
        // chunk must be self-contained: starting with a clock_tms (0x6B) and
        // bounded by the request size. Before the reset fix, each chunk
        // re-sent all prior chunks, so lengths grew without bound.
        let out_size = 16u16;
        let dev = make_dev(out_size, 64);
        let tdi = vec![0xA5u8; 32];
        let tms = vec![0x00u8; 32];
        let mut tdo = vec![0u8; 32];
        dev.shift_chunks(200, &tdi, &tms, &mut tdo).unwrap();

        let sent = dev.handle.received.borrow();
        assert!(
            sent.len() >= 2,
            "expected multiple chunks, got {}",
            sent.len()
        );
        for chunk in sent.iter() {
            assert_eq!(chunk[0], 0x6B, "each chunk must begin with a clock_tms");
            assert!(
                chunk.len() <= 2 * out_size as usize,
                "chunk length {} suggests buffer was not reset",
                chunk.len()
            );
        }
    }
}
