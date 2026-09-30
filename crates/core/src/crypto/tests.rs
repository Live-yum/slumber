use super::*;
use crate::{collection::Collection, render::TemplateContext};
use slumber_template::Template;
use slumber_util::Factory;

fn context(rules: &str) -> TemplateContext {
    let rules = if rules.is_empty() { "      []" } else { rules };
    let yaml = format!(
        "crypto:\n  am:\n    algorithm: aes-128-cbc\n    key: {{value: '0123456789abcdef'}}\n    iv: {{value: '0123456789abcdef'}}\n  b64:\n    algorithm: base64\n  unused:\n    algorithm: aes-128-ecb\n    key: {{value: \"{{{{ env('NOT_SET_FOR_CRYPTO_TEST') }}}}\"}}\nrequests:\n  demo:\n    method: GET\n    url: http://127.0.0.1/\n    response_transform:\n{rules}\n"
    );
    TemplateContext {
        collection: Collection::parse(&yaml).unwrap().into(),
        ..TemplateContext::factory(())
    }
}
#[tokio::test]
async fn templates_and_safe_previews() {
    let context = context("");
    let template: Template =
        "{{ '13800138000' | encrypt('am') | decrypt('am') }}"
            .parse()
            .unwrap();
    assert_eq!(
        template.render_string(&context).await.unwrap(),
        "13800138000"
    );
    let bad: Template = "{{ 12345 | encrypt('am') }}".parse().unwrap();
    assert!(bad.render_bytes(&context).await.is_err());
    let preview = TemplateContext {
        show_sensitive: false,
        ..context
    };
    assert_eq!(
        template.render_string(&preview).await.unwrap(),
        "<sensitive>"
    );
    let response: Template =
        "{{ response('demo', trigger='always', view='transformed') }}"
            .parse()
            .unwrap();
    assert_eq!(
        response.render_string(&preview).await.unwrap(),
        "<sensitive>"
    );
}
#[tokio::test]
async fn transactional_fields_and_large_integers() {
    let context = context(
        "      - type: decrypt\n        crypto: am\n        paths: ['$.list[*].phone', '$.list[0].phone']\n        skip_missing: true\n        skip_null: true\n        skip_blank: true",
    );
    let raw = br#"{"list":[{"phone":"UEI1Z8QQS25WGasdhDJA4g=="},{"phone":null},{"phone":"  \n"},{}],"id":9007199254740993,"huge":1844674407370955161600001}"#;
    let result = transform_response(&context, &"demo".into(), raw)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
    let original: serde_json::Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(json["list"][0]["phone"], "13800138000");
    assert_eq!(json["id"], original["id"]);
    assert_eq!(json["huge"], original["huge"]);
    let bad = br#"{"list":[{"phone":"UEI1Z8QQS25WGasdhDJA4g=="},{"phone":"PRIVATE-BAD-CIPHERTEXT"}]}"#;
    let error = transform_response(&context, &"demo".into(), bad)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("demo")
            && error.contains("am")
            && error.contains("$.list")
    );
    assert!(!error.contains("PRIVATE") && !error.contains("13800138000"));
}
#[tokio::test]
async fn material_recursion_is_not_a_deadlock() {
    let mut context = context("");
    std::sync::Arc::get_mut(&mut context.collection)
        .unwrap()
        .crypto
        .get_mut("am")
        .unwrap()
        .key
        .as_mut()
        .unwrap()
        .value = "{{ secret }}".parse().unwrap();
    context
        .overrides
        .insert("secret".into(), "{{ 'x' | encrypt('am') }}".into());
    assert!(
        context
            .resolve_crypto("am")
            .await
            .unwrap_err()
            .to_string()
            .contains("recursive")
    );
    context
        .overrides
        .insert("secret".into(), "0123456789abcdef".into());
    let first = context
        .resolve_crypto("am")
        .await
        .unwrap()
        .encode(b"test")
        .unwrap();
    context
        .overrides
        .insert("secret".into(), "fedcba9876543210".into());
    assert_ne!(
        first,
        context
            .resolve_crypto("am")
            .await
            .unwrap()
            .encode(b"test")
            .unwrap()
    );
}
