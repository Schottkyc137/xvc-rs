use std::net::SocketAddr;

use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use xvc_server::{
    XvcServer,
    server::{Config, Server},
};

/// A minimal backend that echoes the TCK period and returns zeroed TDO bytes.
pub struct StubBackend;

impl XvcServer for StubBackend {
    fn set_tck(&self, period_ns: u32) -> u32 {
        period_ns
    }

    fn shift(&self, _num_bits: u32, tms: &[u8], _tdi: &[u8]) -> Box<[u8]> {
        vec![0u8; tms.len()].into_boxed_slice()
    }
}

/// Bind to an OS-assigned port, start the server in the background, and return
/// the address and a cancellation token. Drop or cancel the token to shut the
/// server down cleanly.
pub async fn spawn_server(config: Config) -> (SocketAddr, CancellationToken) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let token = CancellationToken::new();
    let server = Server::new(StubBackend, config);
    tokio::spawn({
        let token = token.clone();
        async move {
            server.listen_on(listener, token).await.unwrap();
        }
    });
    (addr, token)
}
