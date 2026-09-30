//! Negative and interoperability tests supplement the independent fixed
//! vectors.
use super::*;
use crate::{collection::Collection, render::TemplateContext};
use base64::{Engine, prelude::BASE64_STANDARD};
use serde_json::json;
use slumber_util::{Factory, yaml::deserialize_str};
use zeroize::Zeroizing;

fn codec(yaml: &str) -> ResolvedCrypto {
    let config: CryptoConfig = deserialize_str(yaml).unwrap();
    let material = |m: Option<&Material>| match m {
        Some(m) => decode_material(
            m.value.display().as_ref(),
            m.encoding,
            config.base64_decode,
        )
        .unwrap(),
        None => Zeroizing::new(Vec::new()),
    };
    let key = material(config.key.as_ref());
    let iv = material(config.iv.as_ref());
    ResolvedCrypto::new(&config, key, iv).unwrap()
}
const AM: &str = "algorithm: aes-128-cbc\nkey: {value: '0123456789abcdef'}\niv: {value: '0123456789abcdef'}";

#[test]
fn byte_encodings_and_exact_key_lengths() {
    for (encoding, value) in [
        ("utf8", "0123456789abcdef"),
        ("text", "0123456789abcdef"),
        ("hex", "30 31 32 33 34 35 36 37 38 39 61 62 63 64 65 66"),
        ("base64", "MDEyMzQ1Njc4OWFiY2RlZg=="),
    ] {
        let config = format!(
            "algorithm: aes-128-cbc\nkey: {{value: '{value}', encoding: {encoding}}}\niv: {{value: '{value}', encoding: {encoding}}}"
        );
        assert_eq!(
            codec(&config).encode(b"13800138000").unwrap(),
            b"UEI1Z8QQS25WGasdhDJA4g=="
        );
    }
    for config in [
        "algorithm: unknown",
        "algorithm: aes-128-cbc",
        "algorithm: base64\nkey: {value: forbidden}",
        "algorithm: aes-128-ecb\nkey: {value: '0123456789abcdef'}\niv: {value: '0123456789abcdef'}",
        "algorithm: aes-128-cbc\nkey: {value: '0123456789abcdef'}\niv: {value: short}",
        "algorithm: aes-128-ecb\nkey: {value: '中文中文中文'}",
        "algorithm: aes-128-ecb\nkey: {value: '0123456789abcdef'}\npadding: none",
    ] {
        assert!(deserialize_str::<CryptoConfig>(config).is_err());
    }
    let error = deserialize_str::<CryptoConfig>(
        "algorithm: aes-128-ecb\nkey: PRIVATE-MATERIAL",
    )
    .unwrap_err()
    .to_string();
    assert!(!error.contains("PRIVATE-MATERIAL"));
}

#[test]
fn all_key_sizes_modes_and_transports() {
    for (bits, key) in [
        (128, "0123456789abcdef"),
        (192, "0123456789abcdef01234567"),
        (256, "0123456789abcdef0123456789abcdef"),
    ] {
        for mode in ["cbc", "ecb"] {
            for transport in ["base64", "base64url"] {
                let iv = if mode == "cbc" {
                    "iv: {value: 'fedcba9876543210'}\n"
                } else {
                    ""
                };
                let codec = codec(&format!(
                    "algorithm: aes-{bits}-{mode}\nkey: {{value: '{key}'}}\n{iv}ciphertext_encoding: {transport}"
                ));
                for input in [
                    b"".as_slice(),
                    b"0123456789abcdef",
                    " 中文\n\r\t ".as_bytes(),
                    b"\xff\x00",
                ] {
                    let encrypted = codec.encode(input).unwrap();
                    assert_eq!(codec.decode(&encrypted).unwrap(), input);
                    if transport == "base64url" {
                        assert!(
                            !encrypted.contains(&b'+')
                                && !encrypted.contains(&b'/')
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn strict_and_relaxed_base64() {
    let strict = codec("algorithm: base64");
    let relaxed = codec(
        "algorithm: base64\nbase64_decode: {ignore_ascii_whitespace: true, allow_missing_padding: true}",
    );
    assert_eq!(
        relaxed.decode(b" a G\r\nV\t s b G8 \x0b\x0c").unwrap(),
        b"hello"
    );
    assert!(strict.decode(b"aGVsbG8").is_err());
    for invalid in [b"a".as_slice(), b"a$==", b"aGVs=bG8", b"====", b"aa==="] {
        assert!(relaxed.decode(invalid).is_err());
    }
    let url = codec(
        "algorithm: base64url\nbase64_decode: {allow_missing_padding: true}",
    );
    assert_eq!(url.encode(b"\xfb\xff").unwrap(), b"-_8=");
    assert_eq!(url.decode(b"+/8").unwrap(), b"\xfb\xff");
    assert_eq!(url.decode(b"-_8").unwrap(), b"\xfb\xff");
    assert!(strict.decode(b"-_8=").is_err());
    assert_eq!(
        codec("algorithm: none").decode(b"\xff \r\n").unwrap(),
        b"\xff \r\n"
    );
}

#[test]
fn invalid_block_length_and_padding() {
    let codec = codec(AM);
    for invalid in [b"".as_slice(), b"!", b"YQ=="] {
        assert!(codec.decode(invalid).is_err());
    }
    let mut ciphertext = BASE64_STANDARD
        .decode(codec.encode(b"1234567890abcdef").unwrap())
        .unwrap();
    ciphertext[15] ^= 0x10; // Corrupt the second block's PKCS7 via the preceding CBC block.
    assert!(
        codec
            .decode(BASE64_STANDARD.encode(ciphertext).as_bytes())
            .is_err()
    );
}

fn context(rules: &str) -> TemplateContext {
    let am = AM
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let yaml = format!(
        "crypto:\n  am:\n{am}  wire: {{algorithm: base64}}\nrequests:\n  demo:\n    method: GET\n    url: http://127.0.0.1/\n    response_transform:\n{rules}"
    );
    TemplateContext {
        collection: Collection::parse(&yaml).unwrap().into(),
        ..TemplateContext::factory(())
    }
}

#[tokio::test]
async fn parse_json_string_and_body_before_fields() {
    let context = context(
        "      - type: decode\n        crypto: wire\n        scope: body\n        parse: json\n        text_encoding: utf8-sig\n      - type: parse_json\n        paths: ['$.list']\n      - type: decrypt\n        crypto: am\n        paths: ['$.list[*].phone']",
    );
    let plain = json!({"list": r#"[{"phone":"UEI1Z8QQS25WGasdhDJA4g=="}]"#, "id": 9007199254740993u64});
    let mut bytes = b"\xef\xbb\xbf".to_vec();
    bytes.extend(serde_json::to_vec(&plain).unwrap());
    let raw = BASE64_STANDARD.encode(bytes);
    let output = transform_response(&context, &"demo".into(), raw.as_bytes())
        .await
        .unwrap();
    let output: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(output["list"][0]["phone"], "13800138000");
    assert_eq!(output["id"], plain["id"]);
}

#[tokio::test]
async fn strict_types_skip_policies_and_utf8() {
    for (flags, input, success) in [
        ("", "{}", false),
        ("skip_missing: true", "{}", true),
        ("skip_missing: true", "{\"phone\":null}", false),
        ("skip_null: true", "{\"phone\":null}", true),
        ("skip_blank: true", "{\"phone\":\" \"}", true),
        ("skip_blank: true", "{\"phone\":7}", false),
        ("skip_missing: true", "[]", false),
    ] {
        let context = context(&format!(
            "      - type: decrypt\n        crypto: am\n        paths: ['$.phone']\n        {flags}"
        ));
        assert_eq!(
            transform_response(&context, &"demo".into(), input.as_bytes())
                .await
                .is_ok(),
            success
        );
    }
    for invalid in ["$..phone", "$.x[?(@.a)]", "$.x[-1]", "$.x[", "$.x.*"] {
        assert!(super::path::FieldPath::parse(invalid).is_err());
    }
    let context = context(
        "      - type: decrypt\n        crypto: am\n        paths: ['$']",
    );
    let encrypted =
        String::from_utf8(codec(AM).encode(b"\xff").unwrap()).unwrap();
    let raw = serde_json::to_vec(&encrypted).unwrap();
    assert!(
        transform_response(&context, &"demo".into(), &raw)
            .await
            .unwrap_err()
            .to_string()
            .contains("UTF-8")
    );
}

/// Keep the shipped token predicate within the built-in jq function subset.
#[tokio::test]
async fn demo_login_token_predicate() {
    let filter = r#"if .code == 0 and (.token | type) == "string" and .token != "" then .token else error("invalid login response") end"#;
    let context = TemplateContext::factory(());
    for (input, success) in [
        (r#"{"code":0,"token":"DEMO-TOKEN"}"#, true),
        (r#"{"code":0,"token":""}"#, false),
        (r#"{"code":0,"token":null}"#, false),
        (r#"{"code":0,"token":123}"#, false),
        (r#"{"code":0}"#, false),
        (r#"{"code":1,"token":"DEMO-TOKEN"}"#, false),
    ] {
        let template = slumber_template::Template::function_call(
            "jq",
            [filter.into(), input.into()],
            [],
        );
        let result = template.render_string(&context).await;
        assert_eq!(result.is_ok(), success, "{input}");
        if success {
            assert_eq!(result.unwrap(), "DEMO-TOKEN");
        }
    }
}
