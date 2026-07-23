//! Minimal XVC server with a loopback backend: it echoes TDI back as TDO and
//! accepts (but ignores) TCK changes. It talks the protocol without any
//! hardware, which is handy for trying out a client.
//!
//! Start it, then connect an XVC client in another terminal:
//!
//! ```sh
//! cargo run --example mock_server
//! # elsewhere:
//! cargo run -p xvc-client --example sample_client -- 127.0.0.1:2542
//! ```

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use xvc_server::XvcServer;
use xvc_server::server::{Config, Server};

/// Loopback backend: report the requested TCK unchanged and set TDO = TDI.
struct Loopback;

impl XvcServer for Loopback {
    type Err = Infallible;

    fn set_tck(&self, period_ns: u32) -> Result<u32, Self::Err> {
        Ok(period_ns)
    }

    fn shift(
        &self,
        _num_bits: u32,
        _tms: &[u8],
        tdi: &[u8],
        tdo: &mut [u8],
    ) -> Result<(), Self::Err> {
        tdo.copy_from_slice(tdi);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = Server::new(Loopback, Config::default());
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2542);
    println!("XVC loopback server listening on {addr}");
    server.listen(addr).await?;
    Ok(())
}
