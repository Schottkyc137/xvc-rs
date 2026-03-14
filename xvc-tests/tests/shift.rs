use xvc_client::XvcClient;
use xvc_server::server::Config;
use xvc_tests::spawn_server;

#[tokio::test(flavor = "multi_thread")]
async fn shift_returns_tdo_of_correct_length() {
    let (addr, _token) = spawn_server(Config::default()).await;
    let mut client = XvcClient::connect(addr).await.unwrap();
    let tdo = client.shift(8, &[0x00], &[0xFF]).await.unwrap();
    assert_eq!(tdo.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn shift_non_byte_aligned_rounds_up() {
    let (addr, _token) = spawn_server(Config::default()).await;
    let mut client = XvcClient::connect(addr).await.unwrap();
    let tdo = client.shift(9, &[0x00, 0x00], &[0xFF, 0xFF]).await.unwrap();
    assert_eq!(tdo.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn shift_multiple_times_in_sequence() {
    let (addr, _token) = spawn_server(Config::default()).await;
    let mut client = XvcClient::connect(addr).await.unwrap();
    for bits in [1u32, 7, 8, 9, 32] {
        let num_bytes = bits.div_ceil(8) as usize;
        let tms = vec![0u8; num_bytes];
        let tdi = vec![0u8; num_bytes];
        let tdo = client.shift(bits, &tms, &tdi).await.unwrap();
        assert_eq!(tdo.len(), num_bytes, "wrong TDO length for {bits} bits");
    }
}
