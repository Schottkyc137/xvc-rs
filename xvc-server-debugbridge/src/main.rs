//! # XVC Server for the AMD Debug Bridge
//!
//! Linux-specific backend implementations for the XVC (Xilinx Virtual Cable) server,
//! providing drivers for various hardware debug interfaces.
//!
//! ## Overview
//!
//! This crate extends [`xvc_server`](https://docs.rs/xvc-server/) with concrete implementations
//! for Linux platforms. It provides three backend drivers:
//!
//! - **kernel-driver**: communicates via the Xilinx kernel driver (`/dev/xilinx_xvc_driver`)
//! - **uio-driver**: memory-mapped access via a userspace I/O device (`/dev/uioN`)
//! - **dev-mem-driver**: memory-mapped access via `/dev/mem` at a given physical address
pub mod backends;

use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use clap_num::maybe_hex;
use env_logger::Env;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use xvc_server::{
    XvcServer,
    server::{Config, Server},
};

const DEFAULT_TIMEOUT_US: u64 = 1000;

#[derive(Parser, Eq, PartialEq, Clone)]
#[allow(clippy::enum_variant_names)]
enum DeviceImpl {
    KernelDriver {
        path: Option<PathBuf>,
    },
    UioDriver {
        path: Option<PathBuf>,
        #[arg(
            short,
            long,
            help = "The timeout in microseconds",
            default_value = "1000"
        )]
        poll_timeout_us: u64,
    },
    DevMemDriver {
        /// Start address of the memory mapped region
        #[clap(value_parser=maybe_hex::<u64>)]
        address: u64,
        #[arg(
            short,
            long,
            help = "The timeout in microseconds",
            default_value = "1000"
        )]
        poll_timeout_us: u64,
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
}

#[derive(Parser)]
#[command(about = "Xilinx Virtual Cable (XVC) JTAG interface for ZynqMP", long_about=None, version)]
struct Args {
    #[arg(short, long, default_value = "2542")]
    port: u16,

    #[arg(short, long, default_value = "0.0.0.0")]
    ip: IpAddr,

    #[clap(subcommand)]
    device: Option<DeviceImpl>,
}

async fn run<T: XvcServer + Send + 'static>(
    backend: T,
    config: Config,
    listener: TcpListener,
    token: CancellationToken,
) -> std::io::Result<()> {
    Server::new(backend, config)
        .listen_on(listener, token)
        .await
}

/// Attempts to automatically find the path to the Debug Bridge kernel driver
fn kernel_driver_path() -> Option<PathBuf> {
    let p = PathBuf::from("/dev/xilinx_xvc_driver");
    if p.exists() { Some(p) } else { None }
}

/// Attempts to automatically find the path to the Debug Bridge via the UIO driver
fn uio_driver_path() -> Option<PathBuf> {
    let uio_class_path = Path::new("/sys/class/uio");
    for entry in uio_class_path.read_dir().ok()? {
        use std::fs;

        let mut path = entry.ok()?.path();
        log::debug!("Looking at UIO path {}", path.display());
        path.push("name");
        let name = match fs::read_to_string(&path) {
            Ok(name) => name,
            Err(_) => continue,
        };
        let uio_name = name.trim();
        log::debug!("UIO has name {}", uio_name);
        if uio_name == "debug_bridge" {
            // This will be something like 'uio2'
            let uio_indexed_name = path.parent()?.file_name()?;
            let mut dev_path = PathBuf::from("/dev");
            // This will be something like '/dev/uio2'
            dev_path.push(uio_indexed_name);
            return Some(dev_path);
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting XVC server");

    let args = Args::parse();
    log::debug!("Parsed arguments: ip={}, port={}", args.ip, args.port);

    let config = Config::default();
    log::debug!("Server config: max_vector_size={}", config.max_vector_size);

    let addr = SocketAddr::new(args.ip, args.port);

    let device_impl = args.device.or_else(|| {
        if let Some(path) = kernel_driver_path() {
            log::info!("Auto-detected Kernel driver at {}", path.display());
            Some(DeviceImpl::KernelDriver { path: Some(path) })
        } else if let Some(path) = uio_driver_path() {
            log::info!("Auto-detected UIO driver at {}", path.display());
            Some(DeviceImpl::UioDriver {
                path: Some(path),
                poll_timeout_us: DEFAULT_TIMEOUT_US,
            })
        } else {
            None
        }
    });

    let Some(device_impl) = device_impl else {
        println!(
            "No debug bridge could be auto detected. Use xvc-server kernel-driver <path>, xvc-server uio-driver <path>, or xvc-server dev-mem-driver <address> to manually specify a driver."
        );
        return Ok(());
    };

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

    match device_impl {
        DeviceImpl::KernelDriver { path } => {
            use crate::backends::kernel_driver::KernelDriverBackend;

            let device_path = match path.or_else(kernel_driver_path) {
                None => {
                    println!(
                        "No debug bridge could be detected. Explicitly specify a path using xvc-server kernel-driver <path> to manually specify a driver."
                    );
                    return Ok(());
                }
                Some(path) => path,
            };
            log::info!(
                "Initializing kernel driver backend from {}",
                device_path.display()
            );
            run(
                KernelDriverBackend::new(device_path)?,
                config,
                listener,
                token,
            )
            .await?;
        }
        DeviceImpl::UioDriver {
            path,
            poll_timeout_us,
        } => {
            use crate::backends::uio::UioDriverBackend;

            let uio_path = match path.or_else(uio_driver_path) {
                None => {
                    println!(
                        "No debug bridge could be detected. Explicitly specify a path using xvc-server uio-driver <path> to manually specify a driver."
                    );
                    return Ok(());
                }
                Some(path) => path,
            };
            log::info!(
                "Initializing UIO driver backend from {}",
                uio_path.display()
            );
            run(
                UioDriverBackend::new(uio_path, Duration::from_micros(poll_timeout_us))?,
                config,
                listener,
                token,
            )
            .await?;
        }
        DeviceImpl::DevMemDriver {
            path,
            address,
            poll_timeout_us,
        } => {
            use crate::backends::devmem::DevMemBackend;

            let poll_timeout = Duration::from_micros(poll_timeout_us);
            let dev_mem = match path {
                Some(path) => DevMemBackend::new_with_path(path, address as i64, poll_timeout),
                None => DevMemBackend::new(address as i64, poll_timeout),
            }?;
            log::info!(
                "Initializing DevMem driver backend using address 0x{:.x}",
                address
            );
            run(dev_mem, config, listener, token).await?;
        }
    }
    Ok(())
}
