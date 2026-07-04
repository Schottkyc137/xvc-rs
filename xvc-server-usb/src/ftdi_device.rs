use std::{fmt::Display, mem::take, time::Duration};

use ftdi_mpsse::{ClockBits, ClockData, ClockTMS, MpsseCmdBuilder, mpsse};
use rusb::{Context, Device, DeviceHandle, UsbContext, constants::LIBUSB_CLASS_PER_INTERFACE};

const FTDI_VID: u16 = 0x0403;

// see https://ftdichip.com/Documents/TechnicalNotes/TN_100_USB_VID-PID_Guidelines.pdf
const KNOWN_PRODUCT_IDS: &[u16] = &[
    0x6010, // FT2232H
    0x6011, // FT4232H
    0x6014, // FT232H
];

/// The chip model name for a known FTDI product id.
fn chip_model(product_id: u16) -> Option<&'static str> {
    match product_id {
        0x6010 => Some("FT2232H"),
        0x6011 => Some("FT4232H"),
        0x6014 => Some("FT232H"),
        _ => None,
    }
}

/// Maximum number of consecutive status-only (no-payload) bulk reads to tolerate
/// in [`FtdiJtagDevice::read`] before giving up on a stalled device.
const MAX_EMPTY_READS: u32 = 1024;

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

pub struct DeviceInfo {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub product_id: u16,
    pub bus_number: u8,
    pub address: u8,
}

impl DeviceInfo {
    /// The chip model name (e.g. "FT2232H"), if the product id is known.
    pub fn chip_model(&self) -> Option<&'static str> {
        chip_model(self.product_id)
    }
}

impl Display for DeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.product.as_deref().unwrap_or("FTDI device"))?;

        let mut details = Vec::new();
        if let Some(chip) = self.chip_model() {
            details.push(chip.to_owned());
        }
        if let Some(manufacturer) = &self.manufacturer {
            details.push(format!("by {manufacturer}"));
        }
        if let Some(serial) = &self.serial {
            details.push(format!("serial {serial}"));
        }
        details.push(format!(
            "bus {:03} device {:03}",
            self.bus_number, self.address
        ));

        write!(f, " ({})", details.join(", "))
    }
}

fn check_device(
    device: Device<rusb::Context>,
    ftdi_port: usize,
    timeout: Duration,
) -> rusb::Result<Option<FtdiJtagDevice>> {
    let descriptor = device.device_descriptor()?;

    if descriptor.class_code() != LIBUSB_CLASS_PER_INTERFACE {
        log::trace!(
            "Rejecting {:?}: wrong class {}",
            device,
            descriptor.class_code()
        );
        return Ok(None);
    }

    if descriptor.vendor_id() != FTDI_VID {
        log::trace!(
            "Rejecting {:?}: vendor id (0x{:x}) not FTDI id",
            device,
            descriptor.vendor_id()
        );
        return Ok(None);
    }

    if !KNOWN_PRODUCT_IDS.contains(&descriptor.product_id()) {
        log::trace!(
            "Rejecting {:?}: product id (0x{:x}) not known FTDI id",
            device,
            descriptor.product_id()
        );
        return Ok(None);
    }

    let dev_config = device
        .active_config_descriptor()
        .or(device.config_descriptor(0))?;

    let Some(iface) = dev_config.interfaces().nth(ftdi_port) else {
        log::trace!(
            "Rejecting {:?}: too few interfaces: requested at index {}, available {}",
            device,
            ftdi_port,
            dev_config.num_interfaces()
        );
        return Ok(None);
    };

    // FTDI has only one descriptor
    let Some(iface_desc) = iface.descriptors().next() else {
        log::warn!("Unsupported FTDI device: No interface descriptors");
        return Ok(None);
    };
    let mut output_ep = None;
    let mut input_ep = None;
    for ep in iface_desc.endpoint_descriptors() {
        match (ep.transfer_type(), ep.direction()) {
            (rusb::TransferType::Bulk, rusb::Direction::In) => {
                if input_ep.is_none() {
                    input_ep = Some(ep)
                } else {
                    log::warn!("Unsupported FTDI device: too many input endpoints");
                    return Ok(None);
                }
            }
            (rusb::TransferType::Bulk, rusb::Direction::Out) => {
                if output_ep.is_none() {
                    output_ep = Some(ep);
                } else {
                    log::warn!("Unsupported FTDI device: too many output endpoints");
                    return Ok(None);
                }
            }
            _ => {}
        }
    }

    let Some(output_ep) = output_ep else {
        log::warn!("Unsupported FTDI device: no output endpoints");
        return Ok(None);
    };
    let Some(input_ep) = input_ep else {
        log::warn!("Unsupported FTDI device: no input endpoints");
        return Ok(None);
    };

    let handle = device.open()?;

    let manufacturer = if let Some(idx) = descriptor.manufacturer_string_index() {
        Some(handle.read_string_descriptor_ascii(idx)?)
    } else {
        None
    };
    let product = if let Some(idx) = descriptor.product_string_index() {
        Some(handle.read_string_descriptor_ascii(idx)?)
    } else {
        None
    };
    let serial_number = if let Some(idx) = descriptor.serial_number_string_index() {
        Some(handle.read_string_descriptor_ascii(idx)?)
    } else {
        None
    };

    let info = DeviceInfo {
        manufacturer,
        product,
        serial: serial_number,
        product_id: descriptor.product_id(),
        bus_number: device.bus_number(),
        address: device.address(),
    };

    Ok(Some(FtdiJtagDevice::new(
        handle,
        info,
        iface.number(),
        BulkEndpoint {
            address: output_ep.address(),
            request_size: output_ep.max_packet_size(),
        },
        BulkEndpoint {
            address: input_ep.address(),
            request_size: input_ep.max_packet_size(),
        },
        timeout,
    )))
}

pub fn list_available_devices(
    ftdi_port: usize,
    timeout: Duration,
) -> rusb::Result<Vec<FtdiJtagDevice>> {
    let ctx = rusb::Context::new()?;

    let mut available_devices = Vec::new();

    for device in ctx.devices()?.iter() {
        let (bus, address) = (device.bus_number(), device.address());
        match check_device(device, ftdi_port, timeout) {
            Ok(Some(device)) => available_devices.push(device),
            Ok(None) => {}
            Err(e) => {
                log::warn!("Skipping USB device (bus {bus:03} device {address:03}): {e}");
            }
        }
    }

    Ok(available_devices)
}

/// A bulk endpoint address paired with its maximum packet (request) size.
pub struct BulkEndpoint {
    pub address: u8,
    pub request_size: u16,
}

pub struct FtdiJtagDevice<H: UsbHandle = DeviceHandle<Context>> {
    handle: H,
    info: DeviceInfo,
    iface: u8,
    bulk_out: BulkEndpoint,
    bulk_in: BulkEndpoint,
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
    fn claim_interface(&self) -> rusb::Result<()> {
        match self.handle.set_auto_detach_kernel_driver(true) {
            Ok(()) | Err(rusb::Error::NotSupported) => {}
            Err(other) => return Err(other),
        }
        self.handle.claim_interface(self.iface)?;

        Ok(())
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
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
        self.claim_interface()?;
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
        info: DeviceInfo,
        iface: u8,
        bulk_out: BulkEndpoint,
        bulk_in: BulkEndpoint,
        timeout: Duration,
    ) -> FtdiJtagDevice<H> {
        FtdiJtagDevice {
            handle,
            info,
            iface,
            bulk_out,
            bulk_in,
            timeout,
        }
    }

    pub fn write(&self, mut values: &[u8]) -> rusb::Result<()> {
        while !values.is_empty() {
            let written = self
                .handle
                .write(self.bulk_out.address, values, self.timeout)?;
            values = &values[written..];
        }
        Ok(())
    }

    pub fn read(&self, out: &mut [u8]) -> rusb::Result<()> {
        let packet = self.bulk_in.request_size as usize;
        let mut buf = vec![0u8; packet];
        let mut filled = 0;

        // The FTDI bulk-IN endpoint emits a 2-byte modem-status packet every
        // latency-timer period whether or not there is payload, so a stalled or
        // disconnected device keeps returning status-only reads without ever
        // erroring. Give up once that many consecutive reads carry no payload.
        let mut empty_reads = 0;

        while filled < out.len() {
            let n = self
                .handle
                .read(self.bulk_in.address, &mut buf, self.timeout)?;
            let before = filled;
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

            if filled > before {
                empty_reads = 0;
            } else {
                empty_reads += 1;
                if empty_reads >= MAX_EMPTY_READS {
                    return Err(rusb::Error::Timeout);
                }
            }
        }
        Ok(())
    }

    // This implementation is mainly translated from https://github.com/BerkeleyLab/XVC-FTDI-JTAG/blob/9633c44ee3c282e9745278af8fcc3e497d178d9b/ftdiJTAG.c#L594
    pub fn shift_chunks(
        &self,
        mut num_bits: u32,
        tdi: &[u8],
        tms: &[u8],
        tdo: &mut [u8],
    ) -> rusb::Result<()> {
        assert!(tdi.len() == tms.len());
        assert!(num_bits.div_ceil(8) as usize == tdi.len());

        let mut tdi_bit = 0x01;
        let mut tdi_index = 0;
        let mut tdo_bit = 0x01;
        let mut tdo_index = 0;
        let mut rx_bitcounts = Vec::new();

        while num_bits != 0 {
            let mut builder = MpsseCmdBuilder::new();
            let mut rx_bytes_wanted = 0u32;

            loop {
                // Stash TMS bits until bit limit reached or TDI would change state
                let tdi_first_state = (tdi[tdi_index] & tdi_bit) != 0;
                let mut cmd_bitcount = 0;
                let mut cmd_bit = 0x01;
                let mut tms_bits = 0;

                let tms_bit = loop {
                    let tms_bit = if (tms[tdi_index] & tdi_bit) != 0 {
                        cmd_bit
                    } else {
                        0
                    };
                    tms_bits |= tms_bit;
                    if tdi_bit == 0x80 {
                        tdi_bit = 0x01;
                        tdi_index += 1;
                    } else {
                        tdi_bit <<= 1;
                    }
                    cmd_bitcount += 1;
                    cmd_bit <<= 1;
                    if !((cmd_bitcount < 6)
                        && (cmd_bitcount < num_bits)
                        && (((tdi[tdi_index] & tdi_bit) != 0) == tdi_first_state))
                    {
                        break tms_bit;
                    }
                };

                /*
                 * Duplicate the final TMS bit so the TMS pin holds
                 * its value for subsequent TDI shift commands.
                 * This is why the bit limit above is 6 and not 7 since
                 * we need space to hold the copy of the final bit.
                 */
                tms_bits |= tms_bit << 1;
                let tms_state = tms_bit != 0;

                /*
                 * Send the TMS bits and TDI value.
                 */
                builder = builder.clock_tms(
                    ClockTMS::NegTMSPosTDO,
                    tms_bits,
                    tdi_first_state,
                    cmd_bitcount as u8, // <= 6 here
                );
                rx_bitcounts.push(cmd_bitcount);
                rx_bytes_wanted += 1;
                num_bits -= cmd_bitcount;

                /*
                 * Stash TDI bits until bit limit reached
                 * or TMS change of state
                 * or transmitter buffer capacity reached.
                 */
                cmd_bitcount = 0;
                let mut cmd_bit = 0x01;
                let mut cmd_index: usize = 0;
                let mut buf = vec![0u8; self.bulk_out.request_size as usize];
                buf[0] = 0;
                while (num_bits != 0)
                    && (((tms[tdi_index] & tdi_bit) != 0) == tms_state)
                    && (((builder.as_slice().len() + (cmd_bitcount as usize / 8)) as u16)
                        < (self.bulk_out.request_size - 5))
                {
                    if (tdi[tdi_index] & tdi_bit) != 0 {
                        buf[cmd_index] |= cmd_bit;
                    }
                    if cmd_bit == 0x80 {
                        cmd_bit = 0x01;
                        cmd_index += 1;
                        buf[cmd_index] = 0;
                    } else {
                        cmd_bit <<= 1;
                    }
                    if tdi_bit == 0x80 {
                        tdi_bit = 0x01;
                        tdi_index += 1;
                    } else {
                        tdi_bit <<= 1;
                    }
                    cmd_bitcount += 1;
                    num_bits -= 1;
                }

                /*
                 * Send stashed TDI bits
                 */
                if cmd_bitcount > 0 {
                    let cmd_bytes = cmd_bitcount / 8;
                    rx_bitcounts.push(cmd_bitcount);
                    if cmd_bitcount >= 8 {
                        rx_bytes_wanted += cmd_bytes;
                        cmd_bitcount -= cmd_bytes * 8;
                        builder =
                            builder.clock_data(ClockData::LsbPosIn, &buf[..cmd_bytes as usize]);
                    }
                    if cmd_bitcount != 0 {
                        rx_bytes_wanted += 1;
                        builder = builder.clock_bits(
                            ClockBits::LsbPosIn,
                            buf[cmd_bytes as usize],
                            cmd_bitcount as u8, // < 8 here (remainder after whole bytes)
                        );
                    }
                }

                if !((num_bits != 0)
                    && (((builder.as_slice().len() + (cmd_bitcount as usize / 8)) as u16)
                        < (self.bulk_out.request_size - 6)))
                {
                    break;
                }
            }

            /*
             * Shift
             */
            self.write(builder.send_immediate().as_slice())?;
            let mut rx_buf = vec![0u8; rx_bytes_wanted as usize];
            self.read(&mut rx_buf)?;

            /*
             * Process received data
             */
            let mut rx_index: usize = 0;
            for mut rx_bitcount in take(&mut rx_bitcounts) {
                let mut rx_bit: u8 = if rx_bitcount < 8 {
                    0x1 << (8 - rx_bitcount)
                } else {
                    0x01
                };
                while rx_bitcount != 0 {
                    rx_bitcount -= 1;
                    if tdo_bit == 0x1 {
                        tdo[tdo_index] = 0;
                    }
                    if (rx_buf[rx_index] & rx_bit) != 0 {
                        tdo[tdo_index] |= tdo_bit;
                    }
                    if rx_bit == 0x80 {
                        rx_index += 1;
                        if rx_bitcount != 0 {
                            rx_bit = if rx_bitcount < 8 {
                                0x1 << (8 - rx_bitcount)
                            } else {
                                0x01
                            };
                        }
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

            if rx_index != rx_bytes_wanted as usize {
                log::warn!("consumed {} but supplied {}", rx_index, rx_bytes_wanted);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::{cell::RefCell, time::Duration};

    use crate::ftdi_device::{BulkEndpoint, DeviceInfo, FtdiJtagDevice, UsbHandle};

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
            info: DeviceInfo {
                manufacturer: Some("company".to_owned()),
                product: Some("product".to_owned()),
                serial: None,
                product_id: 0x6010,
                bus_number: 1,
                address: 4,
            },
            iface: 0,
            bulk_out: BulkEndpoint {
                address: 0x02,
                request_size: out_size,
            },
            bulk_in: BulkEndpoint {
                address: 0x81,
                request_size: in_size,
            },
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn one_tms_bit() {
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 1];
        dev.shift_chunks(1, &[0x01], &[0x00], &mut tdo).unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent.len(), 1, "expected a single chunk");
        assert_eq!(sent[0], [0x6B, 0x00, 0x80, 0x87]);
    }

    #[test]
    fn three_tms_bits() {
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 1];
        dev.shift_chunks(3, &[0x00], &[0x05], &mut tdo).unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent[0], [0x6B, 0x02, 0x0D, 0x87]);
    }

    #[test]
    fn tms_then_tdi_bits() {
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 1];
        dev.shift_chunks(4, &[0x0A], &[0x00], &mut tdo).unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent[0], [0x6B, 0x00, 0x00, 0x3B, 0x02, 0x05, 0x87]);
    }

    #[test]
    fn tms_then_tdi_byte() {
        let dev = make_dev(512, 512);
        let mut tdo = [0u8; 2];
        dev.shift_chunks(9, &[0xFE, 0x01], &[0x00, 0x00], &mut tdo)
            .unwrap();

        let sent = dev.handle.received.borrow();
        assert_eq!(sent[0], [0x6B, 0x00, 0x00, 0x39, 0x00, 0x00, 0xFF, 0x87]);
    }

    #[test]
    fn single_chunk_over_255_bits_does_not_overflow() {
        let dev = make_dev(512, 512);
        let num_bits = 300u32;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tdi = vec![0xA5u8; num_bytes];
        let tms = vec![0x00u8; num_bytes];
        let mut tdo = vec![0u8; num_bytes];
        dev.shift_chunks(num_bits, &tdi, &tms, &mut tdo).unwrap();
    }

    #[test]
    fn large_shift_splits_into_independent_chunks() {
        let out_size = 16u16;
        let dev = make_dev(out_size, 64);
        let num_bits = 256u32;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tdi = vec![0xA5u8; num_bytes];
        let tms = vec![0x00u8; num_bytes];
        let mut tdo = vec![0u8; num_bytes];
        dev.shift_chunks(num_bits, &tdi, &tms, &mut tdo).unwrap();

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
