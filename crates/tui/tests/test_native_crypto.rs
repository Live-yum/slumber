//! Full TUI event-loop tests with real loopback HTTP, without shell decryption.
mod common;
use common::{Runner, TestBackend};
use rstest::rstest;
use slumber_core::database::ProfileFilter;
use slumber_tui::Tui;
use slumber_util::{DataDir, data_dir};
use std::{convert::Infallible, time::Duration};
use terminput::KeyCode;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

async fn screen_contains(
    backend: &TestBackend,
    expected: &str,
) -> Result<(), Infallible> {
    loop {
        let text = backend
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        if text.contains(expected) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[rstest]
#[tokio::test]
async fn native_crypto_tui_fields_raw_history_and_reload(data_dir: DataDir) {
    let server = MockServer::start().await;
    let raw = r#"{"phone":"UEI1Z8QQS25WGasdhDJA4g==","id":9007199254740993}"#;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/echo"))
        .and(matchers::header("x-encrypted", "7Uf+4FRcP6fdBw1EuG6Y2Q=="))
        .and(matchers::query_param(
            "encrypted",
            "7Uf+4FRcP6fdBw1EuG6Y2Q==",
        ))
        .and(matchers::body_json(
            serde_json::json!({"phone":"UEI1Z8QQS25WGasdhDJA4g=="}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(raw)
                .insert_header("Content-Type", "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let yaml = format!(
        r#"
crypto:
  am:
    algorithm: aes-128-cbc
    key: {{value: '0123456789abcdef'}}
    iv: {{value: '0123456789abcdef'}}
requests:
  person:
    method: POST
    url: {}/echo
    headers:
      x-encrypted: "{{{{ '' | encrypt('am') }}}}"
    query:
      encrypted: "{{{{ '' | encrypt('am') }}}}"
    body:
      type: json
      data:
        phone: "{{{{ '13800138000' | encrypt('am') }}}}"
    response_transform:
      - type: decrypt
        crypto: am
        paths: ['$.phone']
"#,
        server.uri()
    );
    let path = data_dir.join("slumber.yml");
    std::fs::write(&path, &yaml).unwrap();
    let backend = TestBackend::new(120, 45);
    let tui = Tui::new(backend.clone(), Some(path.clone())).unwrap();
    let runner = Runner::new(tui)
        .run_until(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok::<_, Infallible>(())
        })
        .await;
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "preview sent a request"
    );
    let runner = runner
        .send_request(0)
        .run_until(screen_contains(&backend, "13800138000"))
        .await
        .send_key(KeyCode::Char('2'))
        .send_key(KeyCode::F(6))
        .run_until(screen_contains(&backend, "Raw network body"))
        .await;
    assert!(
        backend
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("UEI1Z8QQS25WGasdhDJA4g==")
    );
    let tui = runner
        .send_key(KeyCode::F(6))
        .run_until(screen_contains(&backend, "13800138000"))
        .await
        .done()
        .await;
    let stored = tui
        .database()
        .get_latest_request(ProfileFilter::All, &"person".into())
        .unwrap()
        .unwrap();
    assert_eq!(stored.response.body.bytes().as_ref(), raw.as_bytes());
    drop(tui);
    let backend = TestBackend::new(120, 45);
    let tui = Tui::new(backend.clone(), Some(path.clone())).unwrap();
    let runner = Runner::new(tui)
        .run_until(screen_contains(&backend, "13800138000"))
        .await;
    let changed = yaml.replace(
        "key: {value: '0123456789abcdef'}",
        "key: {value: 'fedcba9876543210'}",
    );
    let runner = runner
        .run_until(tokio::fs::write(&path, changed))
        .await
        .run_until(screen_contains(&backend, "Transform FAILED"))
        .await;
    let _tui = runner
        .run_until(tokio::fs::write(&path, yaml))
        .await
        .run_until(screen_contains(&backend, "13800138000"))
        .await
        .done()
        .await;
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[rstest]
#[tokio::test]
async fn native_crypto_tui_whole_body_token_chain(data_dir: DataDir) {
    let server = MockServer::start().await;
    Mock::given(matchers::path("/login"))
        .and(matchers::query_param("user", "demo"))
        .and(matchers::query_param("pass", "public-password"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                "eyJjb2RlIjowLCJ0b2tlbiI6IkRFTU8tVE9LRU4ifQ==",
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(matchers::path("/data"))
        .and(matchers::query_param("token", "DEMO-TOKEN"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    "eyJjb2RlIjowLCJtYXJrZXIiOiJERUNPREVELVZJRVcifQ==",
                )
                .insert_header("Content-Type", "text/plain"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let yaml = format!(
        r#"
crypto:
  wire: {{algorithm: base64}}
profiles:
  demo:
    default: true
    data:
      token: "{{{{ response('login', trigger='no_history', view='transformed') | jsonpath('$.token') | sensitive() }}}}"
requests:
  login:
    method: GET
    url: {}/login
    persist: false
    query: {{user: demo, pass: public-password}}
    response_transform: &decode
      - type: decode
        crypto: wire
        scope: body
        parse: json
        text_encoding: utf8-sig
  data:
    method: GET
    url: {}/data
    persist: false
    query:
      token: "{{{{ token }}}}"
    response_transform: *decode
"#,
        server.uri(),
        server.uri()
    );
    let path = data_dir.join("slumber.yml");
    std::fs::write(&path, yaml).unwrap();
    let backend = TestBackend::new(120, 45);
    let tui = Tui::new(backend.clone(), Some(path)).unwrap();
    let runner = Runner::new(tui)
        .run_until(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok::<_, Infallible>(())
        })
        .await;
    assert!(server.received_requests().await.unwrap().is_empty());
    let tui = runner
        .send_request(1)
        .run_until(screen_contains(&backend, "DECODED-VIEW"))
        .await
        .done()
        .await;
    assert!(
        tui.database()
            .get_latest_request(ProfileFilter::All, &"data".into())
            .unwrap()
            .is_none()
    );
}
