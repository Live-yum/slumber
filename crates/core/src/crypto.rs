//! Native message codecs. CBC/ECB are legacy interoperability modes, not
//! authenticated encryption. Raw HTTP records are never replaced by plaintext.

mod config;
mod engine;
mod path;
#[cfg(test)]
mod tests;
mod transform;
#[cfg(test)]
mod validation;

pub use config::{
    Algorithm, Base64Decode, CryptoConfig, Material, MaterialEncoding,
    ResponseTransform, TextEncoding, TransformKind, TransformScope, Transport,
};
pub use engine::ResolvedCrypto;
pub(crate) use engine::decode_material;
pub use transform::transform_response;

/// Value-free diagnostic: never attach an error chain carrying secret input.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{context}: {message}")]
pub struct CryptoError {
    context: String,
    message: String,
}
impl CryptoError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            context: "crypto".into(),
            message: message.into(),
        }
    }
    pub(crate) fn definition(mut self, id: &str) -> Self {
        self.context = format!("crypto `{id}`");
        self
    }
    pub(crate) fn at(mut self, recipe: &str, path: &str) -> Self {
        self.context =
            format!("recipe `{recipe}`, {}, path `{path}`", self.context);
        self
    }
}
