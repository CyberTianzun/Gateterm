mod common;

use gateterm_cloud_client::credentials::{self, Credentials};
use gateterm_protocol::ServerMsg;
use gateterm_server::cloud::creds::SharedCreds;

async fn hello(addr: std::net::SocketAddr, token: &str) -> ServerMsg {
    common::hello_and_first_control(addr, token).await
}

#[tokio::test]
async fn hello_ok_carries_cloud_server_id_when_logged_in() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cloud.json");
    credentials::save(&path, &Credentials { url: "https://c".into(), email: "a@b.c".into(), token: "t".into(), server_id: Some("srv-1".into()) }).unwrap();
    let addr = common::spawn_server_with_cloud(SharedCreds::new(Some(path)), "tok").await;
    match hello(addr, "tok").await {
        ServerMsg::HelloOk { cloud_server_id, .. } => assert_eq!(cloud_server_id.as_deref(), Some("srv-1")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn hello_ok_has_no_cloud_server_id_when_logged_out() {
    let dir = tempfile::tempdir().unwrap();
    let addr = common::spawn_server_with_cloud(SharedCreds::new(Some(dir.path().join("cloud.json"))), "tok").await;
    match hello(addr, "tok").await {
        ServerMsg::HelloOk { cloud_server_id, .. } => assert_eq!(cloud_server_id, None),
        other => panic!("{other:?}"),
    }
}
