use clap::Parser;
use env_logger::Env;
use inquire::{InquireError, Select};
use std::{
    error::Error,
    fmt::Display,
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

    /// The FTDI port to use
    #[arg(short, long, default_value = "0")]
    ftdi_port: usize,

    /// Debug flag to operate the FTDI chip in loopback mode
    #[arg(short, long, default_value_t = false)]
    loopback: bool,

    /// Whether to present the option interactively
    #[arg(short, long, default_value_t = true)]
    interactive: bool,
}

fn disambiguate_available_devices(
    mut available: Vec<FtdiJtagDevice>,
    interactive: bool,
) -> Option<FtdiJtagDevice> {
    if available.is_empty() {
        return None;
    }
    if available.len() == 1 {
        return Some(available.pop().unwrap());
    }
    if !interactive {
        log::error!("Multiple devices found");
        return None;
    }

    struct Wrapper {
        device: FtdiJtagDevice,
    }

    impl Display for Wrapper {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.device.info())
        }
    }

    let res = Select::new(
        "Multiple matching devices found",
        available
            .into_iter()
            .map(|avail| Wrapper { device: avail })
            .collect(),
    )
    .prompt()
    .inspect_err(|err| {
        if !matches!(
            err,
            InquireError::OperationCanceled | InquireError::OperationInterrupted
        ) {
            log::error!("{err}");
        }
    })
    .ok()?;

    Some(res.device)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    log::debug!("Parsed arguments: ip={}, port={}", args.ip, args.port);

    let config = Config::default();
    log::debug!("Server config: max_vector_size={}", config.max_vector_size);

    let available_devices =
        ftdi_device::list_available_devices(args.ftdi_port, config.read_write_timeout)?;

    let Some(device) = disambiguate_available_devices(available_devices, args.interactive) else {
        return Ok(());
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
