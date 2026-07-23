//! Minimal XVC client: connect to a server, print its capabilities, set the
//! TCK period, and perform a single 8-bit JTAG shift.
//!
//! Run it against any XVC server (defaults to `127.0.0.1:2542`):
//!
//! ```sh
//! cargo run --example sample_client -- 127.0.0.1:2542
//! ```

use std::env;

use xvc_client::XvcClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:2542".to_string());

    let mut client = XvcClient::connect(&addr).await?;
    println!("Connected to {addr}");

    let info = client.get_info().await?;
    println!("Server version:     {}", info.version());
    println!("Max vector length:  {} bytes", info.max_vector_len());

    let period = client.set_tck(10).await?;
    println!("TCK period set to:  {period} ns");

    // Shift 8 bits: hold TMS low and send 0xA5 on TDI.
    let tdo = client.shift(8, &[0x00], &[0xA5]).await?;
    println!("TDO:                {tdo:02x?}");

    Ok(())
}
