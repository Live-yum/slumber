//! Drive the real controller, menus and event loop on each native CI platform.
mod common;
use common::{Runner, TestBackend};
use rstest::rstest;
use slumber_tui::Tui;
use slumber_util::{DataDir, data_dir};
use std::{convert::Infallible, fs, time::Duration};
use terminput::{Event, KeyCode, KeyModifiers};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

async fn visible(
    backend: &TestBackend,
    expected: &str,
) -> Result<(), Infallible> {
    loop {
        if backend
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
            .contains(expected)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
#[rstest]
#[tokio::test]
async fn builtin_x_menu_save_cancel_reload(data_dir: DataDir) {
    let path = data_dir.join("中文 空格 slumber.yml");
    let original = "name: Before\nrequests: {test: {method: GET, url: 'http://localhost/original'}}\n";
    let updated = "name: After\nrequests: {test: {method: GET, url: 'http://localhost/edited'}}\n";
    fs::write(&path, original).unwrap();
    let backend = TestBackend::new(100, 24);
    let tui = Tui::new(backend.clone(), Some(path.clone())).unwrap();
    let runner = Runner::new(tui)
        .action(&[0])
        .run_until(visible(&backend, "Built-in editor"))
        .await
        .send_key_modifiers(KeyCode::Char('a'), KeyModifiers::CTRL)
        .send_input(Event::Paste(updated.into()))
        .send_key(KeyCode::F(2))
        .run_until(visible(&backend, "Reloaded collection"))
        .await;
    assert_eq!(fs::read_to_string(&path).unwrap(), updated);
    let tui = runner
        .send_key(KeyCode::Char('x'))
        .run_until(visible(&backend, "Edit Recipe"))
        .await
        .run_until(async {
            // Keep the menu open while a delayed file-change notification is
            // delivered. It must not reset the view after the first reload.
            tokio::time::sleep(Duration::from_millis(350)).await;
            assert!(
                backend
                    .buffer()
                    .content
                    .iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .contains("Edit Recipe")
            );
            Ok::<(), Infallible>(())
        })
        .await
        .send_key(KeyCode::Enter)
        .run_until(visible(&backend, "Built-in editor"))
        .await
        .send_key_modifiers(KeyCode::Char('a'), KeyModifiers::CTRL)
        .send_input(Event::Paste("DO NOT SAVE".into()))
        .send_key(KeyCode::Esc)
        .run_until(visible(&backend, "Unsaved changes"))
        .await
        .send_key(KeyCode::Char('y'))
        .run_until(visible(&backend, "Editor closed"))
        .await
        .done()
        .await;
    assert_eq!(fs::read_to_string(path).unwrap(), updated);
    assert!(tui.collection().is_some());
}
#[rstest]
#[tokio::test]
async fn builtin_body_edit_pager_query_and_send(data_dir: DataDir) {
    let server = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::body_string("edited-body 中文"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"id": 9_007_199_254_740_993_i64}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let path = data_dir.join("native.yml");
    fs::write(&path, format!("requests:\n  test:\n    method: POST\n    url: {}\n    body: original-body\n", server.uri())).unwrap();
    let backend = TestBackend::new(100, 24);
    let tui = Tui::new(backend.clone(), Some(path.clone())).unwrap();
    let runner = Runner::new(tui)
        .send_keys([KeyCode::Char('1'), KeyCode::Right, KeyCode::Char('e')])
        .run_until(visible(&backend, "Built-in editor"))
        .await
        .send_key_modifiers(KeyCode::Char('a'), KeyModifiers::CTRL)
        .send_input(Event::Paste("edited-body 中文".into()))
        .send_key(KeyCode::F(2))
        .run_until(visible(&backend, "edited-body"))
        .await
        .send_key(KeyCode::Char('v'))
        .run_until(visible(&backend, "Built-in viewer"))
        .await
        .send_key(KeyCode::Esc)
        .send_key(KeyCode::Enter)
        .run_until(visible(&backend, "9007199254740993"))
        .await
        .send_key(KeyCode::Char('2'))
        .send_key(KeyCode::Char('/'))
        .send_text("jq .id + 1")
        .send_key(KeyCode::Enter)
        .run_until(visible(&backend, "9007199254740994"))
        .await;
    runner.done().await;
    assert!(fs::read_to_string(path).unwrap().contains("original-body"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
