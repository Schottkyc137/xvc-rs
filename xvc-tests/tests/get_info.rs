use xvc_client::XvcClient;
use xvc_protocol::Version;
use xvc_server::server::Config;
use xvc_tests::spawn_server;

#[tokio::test(flavor = "multi_thread")]
async fn get_info_returns_v1_0() {
    let (addr, _token) = spawn_server(Config::default()).await;
    let mut client = XvcClient::connect(addr).await.unwrap();
    let info = client.get_info().await.unwrap();
    assert_eq!(info.version(), Version::V1_0);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_info_max_vector_len_matches_config() {
    let config = Config {
        max_vector_size: 1024,
        ..Config::default()
    };
    let (addr, _token) = spawn_server(config).await;
    let mut client = XvcClient::connect(addr).await.unwrap();
    let info = client.get_info().await.unwrap();
    assert_eq!(info.max_vector_len(), 1024);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_info_can_be_called_multiple_times() {
    let (addr, _token) = spawn_server(Config::default()).await;
    let mut client = XvcClient::connect(addr).await.unwrap();
    for _ in 0..3 {
        client.get_info().await.unwrap();
    }
}
