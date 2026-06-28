use clap::Parser;
use env_logger::Env;
use std::{
    error::Error,
    net::{IpAddr, SocketAddr},
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use xvc_server::server::{Config, Server};

use crate::ftdi_server::FtdiServer;

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

    #[arg(short, long)]
    loopback: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    log::debug!("Parsed arguments: ip={}, port={}", args.ip, args.port);

    let config = Config::default();
    log::debug!("Server config: max_vector_size={}", config.max_vector_size);

    let mut available_devices =
        ftdi_device::list_available_devices(args.ftdi_port, config.read_write_timeout)?;

    let device = if available_devices.is_empty() {
        log::error!("No FTDI device detected");
        return Ok(());
    } else if available_devices.len() > 1 {
        // TODO: Choose between different devices interactively and / or allow CLI interaction
        unimplemented!("more than one device")
    } else {
        available_devices.pop().unwrap()
    };

    device.ftdi_init(args.loopback)?;
    log::info!("Using {}", device.info());

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

    log::info!("Starting XVC server");

    Server::new(FtdiServer::new(device), config)
        .listen_on(listener, token)
        .await?;

    Ok(())
}
