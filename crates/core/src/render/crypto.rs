//! Secret material uses an effect-free context and an isolated recursion guard.
use super::{TemplateContext, functions};
use crate::{
    collection::ValueTemplate,
    crypto::{CryptoError, Material, ResolvedCrypto, decode_material},
};
use bytes::Bytes;
use slumber_template::{
    Arguments, Context, Identifier, RenderError, Value, ValueStream,
};
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::Zeroizing;

struct MaterialContext<'a> {
    parent: &'a TemplateContext,
    fields: Vec<Identifier>,
    recursive: &'a AtomicBool,
}
fn restricted() -> RenderError {
    RenderError::other(CryptoError::new(
        "key/IV templates allow only profile fields, env and pure string functions",
    ))
}
impl Context<Value> for MaterialContext<'_> {
    async fn get_field(
        &self,
        field: &Identifier,
    ) -> Result<Value, RenderError> {
        if self.fields.contains(field) || self.fields.len() >= 64 {
            self.recursive.store(true, Ordering::Relaxed);
            return Err(restricted());
        }
        let mut fields = self.fields.clone();
        fields.push(field.clone());
        let context = Self {
            parent: self.parent,
            fields,
            recursive: self.recursive,
        };
        match self.parent.get_field_template(field)? {
            ValueTemplate::String(template) => {
                Box::pin(template.render_string(&context))
                    .await
                    .map(Value::String)
            }
            ValueTemplate::Expression(expression) => {
                Box::pin(expression.render(&context)).await
            }
            _ => Err(restricted()),
        }
    }
    async fn call(
        &self,
        name: &Identifier,
        mut arguments: Arguments<'_, Self>,
    ) -> Result<Value, RenderError> {
        if name.as_str() == "sensitive" {
            let value = arguments.pop_position::<Value>()?;
            arguments.ensure_consumed()?;
            return Ok(value);
        }
        let output = match name.as_str() {
            "env" => functions::env(arguments),
            "base64" => functions::base64(arguments),
            "concat" => functions::concat(arguments),
            "join" => functions::join(arguments),
            "replace" => functions::replace(arguments),
            "lower" => functions::lower(arguments),
            "upper" => functions::upper(arguments),
            "trim" => functions::trim(arguments),
            "slice" => functions::slice(arguments),
            "split" => functions::split(arguments),
            "string" => functions::string(arguments),
            "encrypt" | "decrypt" | "encode" | "decode" => {
                self.recursive.store(true, Ordering::Relaxed);
                return Err(restricted());
            }
            _ => return Err(restricted()),
        }?;
        output.resolve().await
    }
}
impl TemplateContext {
    pub(crate) async fn resolve_crypto(
        &self,
        id: &str,
    ) -> Result<ResolvedCrypto, CryptoError> {
        let config = self.collection.crypto.get(id).ok_or_else(|| {
            CryptoError::new("unknown crypto definition").definition(id)
        })?;
        let recursive = AtomicBool::new(false);
        let context = MaterialContext {
            parent: self,
            fields: Vec::new(),
            recursive: &recursive,
        };
        let render = async |material: Option<&Material>,
                            field: &str|
               -> Result<Zeroizing<Vec<u8>>, CryptoError> {
            let Some(material) = material else {
                return Ok(Zeroizing::new(Vec::new()));
            };
            let text = Zeroizing::new(material.value.render_string(&context).await.map_err(|_| CryptoError::new(if recursive.load(Ordering::Relaxed) {
                format!("recursive crypto/profile dependency in {field}.value (value redacted)")
            } else { format!("unable to render {field}.value: use a literal, profile field or env; side effects are forbidden (value redacted)") }).definition(id))?);
            decode_material(&text, material.encoding, config.base64_decode)
                .map_err(|e| e.definition(id))
        };
        let key = render(config.key.as_ref(), "key").await?;
        let iv = render(config.iv.as_ref(), "iv").await?;
        ResolvedCrypto::new(config, key, iv).map_err(|e| e.definition(id))
    }
}

/// Consume arguments without exposing their contents in type errors.
pub(super) async fn call(
    mut args: Arguments<'_, TemplateContext>,
    decode: bool,
    aes_only: bool,
) -> Result<ValueStream, RenderError> {
    let safe_args = || {
        RenderError::other(CryptoError::new(
            "expected crypto name and string/bytes input, without keyword arguments (values redacted)",
        ))
    };
    let context = args.context();
    if !context.show_sensitive {
        return Ok(Value::String("<sensitive>".into()).into());
    }
    let id: String = args.pop_position().map_err(|_| safe_args())?;
    let value: Value = args.pop_position().map_err(|_| safe_args())?;
    args.ensure_consumed().map_err(|_| safe_args())?;
    let bytes = match value {
        Value::String(text) => Bytes::from(text),
        Value::Bytes(bytes) => bytes,
        _ => return Err(safe_args()),
    };
    let operation = async {
        let codec = context.resolve_crypto(&id).await?;
        if aes_only { codec.require_aes()?; }
        let result = if decode { codec.decode(&bytes)? } else { codec.encode(&bytes)? };
        if aes_only {
            let text = if decode { result.as_slice() } else { bytes.as_ref() };
            std::str::from_utf8(text).map_err(|_| CryptoError::new("encrypt/decrypt require valid UTF-8; encode/decode support bytes"))?;
        }
        Ok::<_, CryptoError>(Bytes::from(result))
    }.await;
    operation
        .map(|bytes| Value::Bytes(bytes).into())
        .map_err(|e| RenderError::other(e.definition(&id)))
}
