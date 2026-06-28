use clap::Parser;
use env_logger::Env;
use rusb::{UsbContext, constants::LIBUSB_CLASS_PER_INTERFACE};
use std::{
    error::Error,
    net::{IpAddr, SocketAddr},
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use xvc_server::server::{Config, Server};

use crate::{ftdi_device::FtdiJtagDevice, ftdi_server::FtdiServer};

mod ftdi_device;
mod ftdi_server;

#[derive(Parser)]
#[command(about = "Xilinx Virtual Cable (XVC) JTAG interface via USB", long_about=None, version)]
struct Args {
    #[arg(short, long, default_value = "2542")]
    port: u16,

    #[arg(short, long, default_value = "0.0.0.0")]
    ip: IpAddr,

    #[arg(short, long, default_value = "0")]
    ftdi_port: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting XVC server");

    let args = Args::parse();
    log::debug!("Parsed arguments: ip={}, port={}", args.ip, args.port);

    let config = Config::default();
    log::debug!("Server config: max_vector_size={}", config.max_vector_size);

    // MARK: Get FTDI-USB device

    let ctx = rusb::Context::new()?;

    const FTDI_VID: u16 = 0x0403;

    // see https://ftdichip.com/Documents/TechnicalNotes/TN_100_USB_VID-PID_Guidelines.pdf
    const KNOWN_PRODUCT_IDS: &[u16] = &[
        0x6010, // FT2232H
        0x6011, // FT4232H
        0x6014, // FT232H
    ];

    let mut available_devices = Vec::new();

    println!("Available devices: {}", ctx.devices()?.len());

    for device in ctx.devices()?.iter() {
        let descriptor = device.device_descriptor()?;
        println!("Found device: {:?}", descriptor);

        if descriptor.class_code() != LIBUSB_CLASS_PER_INTERFACE {
            log::trace!(
                "Rejecting {:?}: wrong class {}",
                device,
                descriptor.class_code()
            );
            continue;
        }

        if descriptor.vendor_id() != FTDI_VID {
            log::trace!(
                "Rejecting {:?}: vendor id (0x{:x}) not FTDI id",
                device,
                descriptor.vendor_id()
            );
            continue;
        }

        if !KNOWN_PRODUCT_IDS.contains(&descriptor.product_id()) {
            log::trace!(
                "Rejecting {:?}: product id (0x{:x}) not known FTDI id",
                device,
                descriptor.product_id()
            );
            continue;
        }

        let dev_config = device
            .active_config_descriptor()
            .or(device.config_descriptor(0))?;

        let Some(iface) = dev_config.interfaces().nth(args.ftdi_port) else {
            log::trace!(
                "Rejecting {:?}: too few interfaces: requested at index {}, available {}",
                device,
                args.ftdi_port,
                dev_config.num_interfaces()
            );
            continue;
        };

        // FTDI has only one descriptor
        let iface_desc = iface.descriptors().next().unwrap();
        let mut output_ep = None;
        let mut input_ep = None;
        for ep in iface_desc.endpoint_descriptors() {
            match (ep.transfer_type(), ep.direction()) {
                (rusb::TransferType::Bulk, rusb::Direction::In) => {
                    if input_ep.is_none() {
                        input_ep = Some(ep)
                    } else {
                        panic!("Multiple input endpoints");
                    }
                }
                (rusb::TransferType::Bulk, rusb::Direction::Out) => {
                    if output_ep.is_none() {
                        output_ep = Some(ep);
                    } else {
                        panic!("Multiple output endpoints");
                    }
                }
                _ => {}
            }
        }

        // let handle = device.open()?;
        // let manufacturer = descriptor.serial_number_string_index().map(|idx| handle.read_string_descriptor_ascii(idx));
        // println!("Manufacturer: {}", handle.read_manufacturer_string_ascii(&descriptor)?);
        // println!("Product: {}", handle.read_product_string_ascii(&descriptor)?);
        // println!("S/N: {}", handle.read_serial_number_string_ascii(&descriptor)?);

        let output_ep = output_ep.expect("Expected exactly one output endpoint");
        let input_ep = input_ep.expect("Expected exactly one input endpoint");

        available_devices.push(FtdiJtagDevice::new(
            device.open()?,
            iface.number(),
            output_ep.address(),
            output_ep.max_packet_size(),
            input_ep.address(),
            input_ep.max_packet_size(),
            config.read_write_timeout,
        ));
    }

    // TODO: improve error messages
    let device = if available_devices.is_empty() {
        println!("No device available");
        return Ok(());
    } else if available_devices.len() > 1 {
        // TODO: Choose between different devices interactively and / or allow CLI interaction
        unimplemented!("more than one device")
    } else {
        available_devices.pop().unwrap()
    };

    device.claim_interface()?;
    device.ftdi_init()?;

    // MARK: END

    let addr = SocketAddr::new(args.ip, args.port);

    let listener = TcpListener::bind(addr).await?;
    log::info!("Listening on {}", addr);

    let token = CancellationToken::new();
    tokio::spawn({
        let token = token.clone();
        async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                log::info!("Received Ctrl+C, shutting down gracefully");
                token.cancel();
            }
        }
    });

    Server::new(FtdiServer::new(device), config)
        .listen_on(listener, token)
        .await?;

    Ok(())
}
