# Native Crypto and Portable Mode

This fork adds per-collection native message codecs. No external encryption command, interpreter, or query program is required. Base64 is an encoding, not encryption. AES-CBC/ECB are legacy interoperability modes, not authenticated encryption.

## Configure, send, view

```yaml
crypto:
  legacy:
    algorithm: aes-128-cbc
    padding: pkcs7
    key: {value: '0123456789abcdef', encoding: utf8}
    iv: {value: '0123456789abcdef', encoding: utf8}
    ciphertext_encoding: base64
requests:
  example:
    method: POST
    url: http://127.0.0.1:18080/example
    persist: false
    body:
      type: json
      data:
        phone: "{{ '00000000000' | encrypt('legacy') }}"
    response_transform:
      - type: decrypt
        crypto: legacy
        paths: ['$.people[*].phone']
        skip_missing: true
        skip_null: true
        skip_blank: true
```

The displayed key is public test material. Edit your private collection, select a request in the TUI, and press Enter. The response body automatically displays the configured derived JSON. Focus the response pane and press F6 to switch raw/derived views. The `toggle_response_view` input binding is configurable. Pending/failed transforms are not shown as successfully decrypted content. The existing query, copy and save actions use the selected view.

## Named definitions

| Field | Behavior |
| --- | --- |
| `algorithm` | Required: `none`, `base64`, `base64url`, `aes-128-cbc`, `aes-192-cbc`, `aes-256-cbc`, or corresponding `-ecb` variants |
| `key.value` / `iv.value` | Quoted literal or template; no derivation, trimming, truncation or implicit encoding detection |
| `key.encoding` / `iv.encoding` | `utf8` (default), `text` (UTF-8 alias), `hex`, or `base64` |
| `padding` | AES-only, `pkcs7`; omitted means PKCS7 |
| `plaintext_encoding` | AES-only, `utf8` |
| `ciphertext_encoding` | AES-only, `base64` (default) or `base64url` |
| `base64_decode.ignore_ascii_whitespace` | Default false; explicitly accept ASCII space, tab, CR, LF, VT, FF |
| `base64_decode.allow_missing_padding` | Default false; explicitly supply missing final `=`; invalid characters and lengths still fail |

AES keys decode to exactly 16/24/32 bytes, CBC IVs to 16 bytes. ECB forbids an IV. None/Base64 codecs do not accept AES material fields. Encoding emits padded output without newlines; Base64URL decoding also accepts the standard alphabet. Configured fixed IVs are respected, without a salt/nonce/tag/header prefix. This does not make fixed IVs or KEY=IV recommended protocol designs, and invalid keys cannot always be detected with unauthenticated CBC/ECB.

Material templates support profile fields, `env()`, `sensitive()` and pure string functions. They do not use request caches or permit network, subprocess, file, logging, prompt, or recursive codec dependencies. Unused definitions do not resolve their environment variables. Literal material is redacted in diagnostics.

## Template functions

`encrypt(crypto_id, value)` and `decrypt(crypto_id, value)` require AES and valid UTF-8 text. `encode(crypto_id, value)` and `decode(crypto_id, value)` support all codecs and byte input. Piped input is the last positional argument. Numeric/object/array input is not implicitly serialized.

Use these functions in JSON fields, query parameters and headers. Original JSON escaping and URL encoding are retained. A whole raw body uses the existing string-body syntax:

```yaml
headers: {Content-Type: text/plain}
body: "{{ file('payload.json') | encode('legacy') }}"
```

Do not add JSON quotes around that encoded body. Use this only with an endpoint that explicitly accepts this wire format, not merely one whose responses happen to be encrypted. Unconfigured ordinary JSON requests remain unchanged. Previews mask codec results; sent request records contain the actual wire payload without re-evaluating encryption.

## Response rules

Rules run in order and publish a result only when all rules succeed. The original status, headers, Content-Length and body remain intact. Derived plaintext is not automatically persisted.

| Field | Behavior |
| --- | --- |
| `type` | Required: `decrypt`, `decode`, or `parse_json` |
| `crypto` | Required definition name for decode/decrypt; forbidden for parse_json |
| `scope` | `fields` (default), or `body` only for the first rule |
| `paths` | Required for fields; forbidden for body |
| `skip_missing` | Only absent paths, not incorrect container types |
| `skip_null` | Only null field values |
| `skip_blank` | Only blank string field values; no trimming of other plaintext |
| `parse` | Body rules require `json`; not accepted for fields |
| `text_encoding` | Body only: `utf8` or `utf8-sig` to remove a leading UTF-8 BOM |

The writable JSONPath subset supports `$`, `.field`, `[0]`, `[*]`, and `["special field"]`. Unsupported syntax fails. Duplicate selectors do not decode twice; conflicting operations or skip policies fail. Fields must be strings unless explicitly skipped. Invalid Base64, blocks, padding, UTF-8, JSON, or container types fail the entire transformation without returning partially decoded JSON. Unselected values, including large JSON integers, remain unchanged in value.

A string containing a JSON array must have an explicit `type: parse_json` field rule before selecting its elements. Nothing implicitly parses every string. For a whole encrypted response, configure `scope: body`, `parse: json` and optionally `text_encoding: utf8-sig`; raw bytes are decoded *before* parsing, regardless of the original Content-Type. Additional field rules can follow.

## CLI, chaining and portable mode

CLI output remains raw by default. `slumber request example --transformed --output result.json` explicitly derives the configured view. Transformation failures exit with code 3, do not emit partial JSON, and do not replace an existing output file. `response()` remains raw; explicitly use `response('login', view='transformed')` before querying a token in a whole-body encrypted login response. These paths share one implementation.

`slumber --portable` locates config.yml and the default slumber.yml beside the executable, stores database/UI state in data/state.sqlite, logs in log and temporary files in tmp. `-f` still overrides collection discovery, and file templates remain relative to their collection. Normal mode retains upstream path behavior. Portable mode uses compiled Mozilla public CA certificates with TLS verification enabled; root updates require rebuilding. Optional shell queries, editors and sqlite shell are separate from native request/codec operation.

The complete Chinese deployment guide and local demo service examples are included in this fork's portable archives. Only local mock services and public material are used in automated protocol tests.
