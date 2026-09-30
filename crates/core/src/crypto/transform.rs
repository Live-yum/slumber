use super::{
    CryptoError, ResponseTransform, TextEncoding, TransformKind,
    TransformScope, path::FieldPath,
};
use crate::{collection::RecipeId, render::TemplateContext};
use bytes::Bytes;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

fn parse_json(bytes: &[u8]) -> Result<Value, CryptoError> {
    serde_json::from_slice(bytes).map_err(|e| {
        CryptoError::new(format!(
            "invalid JSON or UTF-8 at line {}, column {}",
            e.line(),
            e.column()
        ))
    })
}

/// A transactional derived view. Never mutates or persists network bytes.
pub async fn transform_response(
    context: &TemplateContext,
    recipe_id: &RecipeId,
    raw: &[u8],
) -> Result<Bytes, CryptoError> {
    let recipe = context
        .collection
        .recipes
        .get_recipe(recipe_id)
        .ok_or_else(|| CryptoError::new("unknown recipe").at(recipe_id, "$"))?;
    if recipe.response_transform.is_empty() {
        return Ok(Bytes::copy_from_slice(raw));
    }
    let mut json: Option<Value> = None;
    let mut codecs = HashMap::new();
    let mut visited: HashMap<String, &ResponseTransform> = HashMap::new();
    for (index, rule) in recipe.response_transform.iter().enumerate() {
        let id = rule.crypto.as_deref().unwrap_or("parse_json");
        let at = |error: CryptoError, path: &str| {
            error.definition(id).at(recipe_id, path)
        };
        if rule.kind != TransformKind::ParseJson && !codecs.contains_key(id) {
            let codec =
                context.resolve_crypto(id).await.map_err(|e| at(e, "$"))?;
            codecs.insert(id, codec);
        }
        let codec = codecs.get(id);
        if rule.kind == TransformKind::Decrypt {
            codec
                .expect("loaded codec")
                .require_aes()
                .map_err(|e| at(e, "$"))?;
        }
        if rule.scope == TransformScope::Body {
            if index != 0 {
                return Err(at(
                    CryptoError::new(
                        "scope: body is allowed only as the first transform",
                    ),
                    "$",
                ));
            }
            let decoded = codec
                .expect("body codec")
                .decode(raw)
                .map_err(|e| at(e, "$"))?;
            let bytes = if rule.text_encoding == Some(TextEncoding::Utf8Sig) {
                decoded.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&decoded)
            } else {
                &decoded
            };
            json = Some(parse_json(bytes).map_err(|e| at(e, "$"))?);
            continue;
        }
        if json.is_none() {
            json = Some(parse_json(raw).map_err(|e| at(e, "$"))?);
        }
        let value = json.as_mut().expect("parsed JSON");
        let mut hits = BTreeMap::new();
        for selector in &rule.paths {
            let path =
                FieldPath::parse(selector).map_err(|e| at(e, selector))?;
            for pointer in path
                .select(value, rule.skip_missing)
                .map_err(|e| at(e, selector))?
            {
                hits.entry(pointer).or_insert(selector.as_str());
            }
        }
        for (pointer, selector) in hits {
            if let Some(previous) = visited.get(&pointer) {
                if previous.kind == rule.kind
                    && previous.crypto == rule.crypto
                    && previous.skip_missing == rule.skip_missing
                    && previous.skip_null == rule.skip_null
                    && previous.skip_blank == rule.skip_blank
                {
                    continue;
                }
                return Err(at(
                    CryptoError::new(
                        "conflicting transforms select the same field",
                    ),
                    selector,
                ));
            }
            visited.insert(pointer.clone(), rule);
            let selected = value
                .pointer_mut(&pointer)
                .expect("selected pointer exists");
            if rule.skip_null && selected.is_null() {
                continue;
            }
            let text = selected.as_str().ok_or_else(|| {
                at(
                    CryptoError::new("selected value must be a string"),
                    selector,
                )
            })?;
            if rule.skip_blank && text.chars().all(char::is_whitespace) {
                continue;
            }
            *selected = if rule.kind == TransformKind::ParseJson {
                parse_json(text.as_bytes()).map_err(|e| at(e, selector))?
            } else {
                let bytes = codec
                    .expect("field codec")
                    .decode(text.as_bytes())
                    .map_err(|e| at(e, selector))?;
                Value::String(String::from_utf8(bytes).map_err(|_| {
                    at(
                        CryptoError::new("decoded field is not valid UTF-8"),
                        selector,
                    )
                })?)
            };
        }
    }
    serde_json::to_vec_pretty(&json.expect("nonempty transform list"))
        .map(Bytes::from)
        .map_err(|_| {
            CryptoError::new("unable to serialize transformed JSON")
                .at(recipe_id, "$")
        })
}
