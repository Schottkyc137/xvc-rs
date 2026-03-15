use xvc_client::XvcClient;
use xvc_server::server::Config;
use xvc_tests::spawn_server;

#[tokio::test(flavor = "multi_thread")]
async fn second_client_is_rejected() {
    let (addr, _token) = spawn_server(Config::default()).await;

    // First client connects and makes a successful request.
    let mut client_a = XvcClient::connect(addr).await.unwrap();
    client_a.get_info().await.unwrap();

    // Second client connects while the first is still active. The server accepts
    // the TCP handshake but immediately closes the connection.
    let mut client_b = XvcClient::connect(addr).await.unwrap();
    assert!(client_b.get_info().await.is_err());

    // First client is unaffected and can continue.
    client_a.get_info().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn new_client_can_connect_after_previous_disconnects() {
    let (addr, _token) = spawn_server(Config::default()).await;

    {
        let mut client_a = XvcClient::connect(addr).await.unwrap();
        client_a.get_info().await.unwrap();
        // client_a dropped here, TCP connection closed
    }

    // Server should accept the next client now that the lock is free.
    let mut client_b = XvcClient::connect(addr).await.unwrap();
    client_b.get_info().await.unwrap();
}
