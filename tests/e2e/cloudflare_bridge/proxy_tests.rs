use super::*;
use crate::test_support::{
    api::{Fault, MockApi, record},
    config,
};
use acme_proxy::{
    config::Rfc2136Config,
    signer::relay::dns01::{DnsUpdater, Rfc2136Updater},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::time::Duration;

const OWNER: &str = "_acme-challenge.example.com.";

async fn updater(api: &MockApi) -> (Rfc2136Updater, tokio::task::JoinHandle<Result<()>>) {
    updater_with_budget(api, 2, Duration::from_secs(2)).await
}

async fn updater_with_budget(
    api: &MockApi,
    timeout_secs: u64,
    api_timeout: Duration,
) -> (Rfc2136Updater, tokio::task::JoinHandle<Result<()>>) {
    let cfg = config();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let proxy = Rfc2136Updater::from_config(&Rfc2136Config {
        server: socket.local_addr().unwrap().to_string(),
        zone: cfg.dns_zone.to_string(),
        tsig_key_name: cfg.tsig_key_name.to_string(),
        tsig_key_secret: STANDARD.encode(&cfg.tsig_secret),
        tsig_algorithm: "hmac-sha256".into(),
        timeout_secs,
    })
    .unwrap();
    let server = tokio::spawn(serve_udp(
        socket,
        Arc::new(cfg),
        Arc::new(api.client_with_timeout(api_timeout)),
    ));
    (proxy, server)
}

#[tokio::test]
async fn proxy_concurrent_values_and_retries_preserve_other_records() {
    let api = MockApi::start(vec![]).await;
    let (proxy, server) = updater(&api).await;
    let (a, b) = tokio::join!(
        proxy.upsert_txt(OWNER, "VaLuE-A"),
        proxy.upsert_txt(OWNER, "value-b")
    );
    a.unwrap();
    b.unwrap();
    proxy.upsert_txt(OWNER, "VaLuE-A").await.unwrap();
    let mut values = api.values();
    values.sort();
    assert_eq!(values, ["\"VaLuE-A\"", "\"value-b\""]);
    proxy.delete_txt(OWNER, "VaLuE-A").await.unwrap();
    proxy.delete_txt(OWNER, "VaLuE-A").await.unwrap();
    assert_eq!(api.values(), ["\"value-b\""]);
    proxy.delete_txt(OWNER, "value-b").await.unwrap();
    assert!(api.values().is_empty());
    server.abort();
}

#[tokio::test]
async fn proxy_rejects_api_failures_without_remote_error_text() {
    for status in [403, 429, 503] {
        let api = MockApi::start(vec![]).await;
        api.state.lock().unwrap().faults.insert(
            1,
            Fault {
                status,
                body: "remote-sensitive-canary".into(),
                delay: Duration::from_millis(10),
            },
        );
        let (proxy, server) = updater(&api).await;
        let error = proxy.upsert_txt(OWNER, "value").await.unwrap_err();
        assert!(error.contains("DNS update refused: Server Failure"));
        assert!(!error.contains("remote-sensitive-canary"));
        assert!(api.values().is_empty());
        proxy.upsert_txt(OWNER, "value").await.unwrap();
        proxy.delete_txt(OWNER, "value").await.unwrap();
        assert!(api.values().is_empty());
        server.abort();
    }
}

#[tokio::test]
async fn proxy_cleanup_retries_after_partial_api_success() {
    let api = MockApi::start(vec![
        record("a", OWNER.trim_end_matches('.'), "TXT", "\"value\""),
        record("b", OWNER.trim_end_matches('.'), "TXT", "\"value\""),
        record("other", OWNER.trim_end_matches('.'), "TXT", "\"other\""),
    ])
    .await;
    api.state.lock().unwrap().faults.insert(
        3,
        Fault {
            status: 429,
            body: "{}".into(),
            delay: Duration::ZERO,
        },
    );
    let (proxy, server) = updater(&api).await;
    assert!(proxy.delete_txt(OWNER, "value").await.is_err());
    assert_eq!(api.values().len(), 2);
    proxy.delete_txt(OWNER, "value").await.unwrap();
    assert_eq!(api.values(), ["\"other\""]);
    server.abort();
}

#[tokio::test]
async fn proxy_retries_when_the_response_is_lost_after_a_write() {
    let api = MockApi::start(vec![]).await;
    let cfg = Arc::new(config());
    let client = Arc::new(api.client());
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let proxy = Rfc2136Updater::from_config(&Rfc2136Config {
        server: socket.local_addr().unwrap().to_string(),
        zone: cfg.dns_zone.to_string(),
        tsig_key_name: cfg.tsig_key_name.to_string(),
        tsig_key_secret: STANDARD.encode(&cfg.tsig_secret),
        tsig_algorithm: "hmac-sha256".into(),
        timeout_secs: 1,
    })
    .unwrap();
    let server = tokio::spawn(async move {
        let mut bytes = [0; 4096];
        for index in 0..3 {
            let (n, peer) = socket.recv_from(&mut bytes).await.unwrap();
            let response = handle_wire(&bytes[..n], cfg.clone(), client.clone()).await;
            if index != 0 {
                socket.send_to(&response, peer).await.unwrap();
            }
        }
    });
    assert!(
        proxy
            .upsert_txt(OWNER, "value")
            .await
            .unwrap_err()
            .contains("timed out")
    );
    assert_eq!(api.values(), ["\"value\""]);
    proxy.upsert_txt(OWNER, "value").await.unwrap();
    assert_eq!(api.values(), ["\"value\""]);
    proxy.delete_txt(OWNER, "value").await.unwrap();
    assert!(api.values().is_empty());
    server.await.unwrap();
}

#[tokio::test]
async fn proxy_waits_for_slow_api_and_rejects_api_timeout() {
    for timeout in [false, true] {
        let api = MockApi::start(vec![]).await;
        api.state.lock().unwrap().faults.insert(1, Fault {
            status: 200,
            body: serde_json::json!({"success":true,"errors":[],"result":[],"result_info":{"page":1,"total_pages":1}}).to_string(),
            delay: if timeout { Duration::from_millis(100) } else { Duration::from_secs(11) },
        });
        let (proxy, server) = updater_with_budget(
            &api,
            15,
            if timeout {
                Duration::from_millis(20)
            } else {
                Duration::from_secs(15)
            },
        )
        .await;
        let result = proxy.upsert_txt(OWNER, "value").await;
        assert_eq!(result.is_err(), timeout);
        proxy.delete_txt(OWNER, "value").await.unwrap();
        assert!(api.values().is_empty());
        server.abort();
    }
}
