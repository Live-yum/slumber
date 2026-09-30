use super::{CryptoError, engine::decode_material, path::FieldPath};
use serde::{Deserialize, Serialize};
use slumber_template::Template;
use slumber_util::yaml::{
    self, DeserializeYaml, Expected, Field, LocatedError, SourceMap,
    SourcedYaml, StructDeserializer, YamlErrorKind,
};
use std::fmt;

macro_rules! yaml_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        pub enum $name {
            $(#[serde(rename = $text)] $variant,)+
        }
        impl DeserializeYaml for $name {
            fn expected() -> Expected {
                Expected::OneOf(&[$(&Expected::Literal($text),)+])
            }
            fn deserialize(yaml: SourcedYaml, _: &SourceMap) -> yaml::Result<Self> {
                let location = yaml.location;
                let text = yaml.try_into_string()?;
                match text.as_str() {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(LocatedError {
                        error: YamlErrorKind::Unexpected {
                            expected: Self::expected(), actual: format!("{text:?}"),
                        },
                        location,
                    }),
                }
            }
        }
    };
}

yaml_enum!(Algorithm {
    None => "none", Base64 => "base64", Base64Url => "base64url",
    Aes128Cbc => "aes-128-cbc", Aes192Cbc => "aes-192-cbc", Aes256Cbc => "aes-256-cbc",
    Aes128Ecb => "aes-128-ecb", Aes192Ecb => "aes-192-ecb", Aes256Ecb => "aes-256-ecb",
});
yaml_enum!(MaterialEncoding { Utf8 => "utf8", Text => "text", Hex => "hex", Base64 => "base64" });
yaml_enum!(Transport { Base64 => "base64", Base64Url => "base64url" });
yaml_enum!(Padding { Pkcs7 => "pkcs7" });
yaml_enum!(PlaintextEncoding { Utf8 => "utf8" });
yaml_enum!(TransformKind { Decode => "decode", Decrypt => "decrypt", ParseJson => "parse_json" });
yaml_enum!(TransformScope { Fields => "fields", Body => "body" });
yaml_enum!(TextEncoding { Utf8 => "utf8", Utf8Sig => "utf8-sig" });
yaml_enum!(ParseAs { Json => "json" });

impl Default for MaterialEncoding {
    fn default() -> Self {
        Self::Utf8
    }
}
impl Default for TransformScope {
    fn default() -> Self {
        Self::Fields
    }
}

impl Algorithm {
    pub fn key_len(self) -> Option<usize> {
        match self {
            Self::Aes128Cbc | Self::Aes128Ecb => Some(16),
            Self::Aes192Cbc | Self::Aes192Ecb => Some(24),
            Self::Aes256Cbc | Self::Aes256Ecb => Some(32),
            _ => None,
        }
    }

    pub fn is_cbc(self) -> bool {
        matches!(self, Self::Aes128Cbc | Self::Aes192Cbc | Self::Aes256Cbc)
    }
}

/// Literal or templated material. Debug output always redacts its source.
#[derive(Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Material {
    pub value: Template,
    #[cfg_attr(feature = "schema", schemars(default))]
    pub encoding: MaterialEncoding,
}

impl fmt::Debug for Material {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Material")
            .field("value", &"[REDACTED]")
            .field("encoding", &self.encoding)
            .finish()
    }
}

impl DeserializeYaml for Material {
    fn expected() -> Expected {
        Expected::Mapping
    }
    fn deserialize(
        yaml: SourcedYaml,
        source_map: &SourceMap,
    ) -> yaml::Result<Self> {
        let mut d = StructDeserializer::new(yaml).map_err(|error| {
            LocatedError::other(CryptoError::new("key/iv must be a mapping with value and encoding (material redacted)"), error.location)
        })?;
        let value = d.get(Field::new("value"), source_map).map_err(|error| {
            LocatedError::other(CryptoError::new(
                "key/iv.value must be a quoted string or valid template (value redacted)"
            ), error.location)
        })?;
        let encoding = d.get(
            Field::new("encoding").or(MaterialEncoding::Utf8),
            source_map,
        )?;
        d.done()?;
        Ok(Self { value, encoding })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Base64Decode {
    #[cfg_attr(feature = "schema", schemars(default))]
    pub ignore_ascii_whitespace: bool,
    #[cfg_attr(feature = "schema", schemars(default))]
    pub allow_missing_padding: bool,
}

impl DeserializeYaml for Base64Decode {
    fn expected() -> Expected {
        Expected::Mapping
    }
    fn deserialize(
        yaml: SourcedYaml,
        source_map: &SourceMap,
    ) -> yaml::Result<Self> {
        let mut d = StructDeserializer::new(yaml)?;
        let value = Self {
            ignore_ascii_whitespace: d
                .get(Field::new("ignore_ascii_whitespace").opt(), source_map)?,
            allow_missing_padding: d
                .get(Field::new("allow_missing_padding").opt(), source_map)?,
        };
        d.done()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CryptoConfig {
    pub algorithm: Algorithm,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<Material>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iv: Option<Material>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plaintext_encoding: Option<PlaintextEncoding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ciphertext_encoding: Option<Transport>,
    #[cfg_attr(feature = "schema", schemars(default))]
    pub base64_decode: Base64Decode,
}

impl CryptoConfig {
    fn validate(&self) -> Result<(), CryptoError> {
        if let Some(len) = self.algorithm.key_len() {
            let key = self
                .key
                .as_ref()
                .ok_or_else(|| CryptoError::new("AES requires key.value"))?;
            if !key.value.is_dynamic() {
                let value = decode_material(
                    key.value.display().as_ref(),
                    key.encoding,
                    self.base64_decode,
                )?;
                if value.len() != len {
                    return Err(CryptoError::new(format!(
                        "key.value must decode to {len} bytes"
                    )));
                }
            }
            if self.algorithm.is_cbc() {
                let iv = self
                    .iv
                    .as_ref()
                    .ok_or_else(|| CryptoError::new("CBC requires iv.value"))?;
                if !iv.value.is_dynamic()
                    && decode_material(
                        iv.value.display().as_ref(),
                        iv.encoding,
                        self.base64_decode,
                    )?
                    .len()
                        != 16
                {
                    return Err(CryptoError::new(
                        "iv.value must decode to 16 bytes",
                    ));
                }
            } else if self.iv.is_some() {
                return Err(CryptoError::new(
                    "ECB does not accept an IV; remove iv",
                ));
            }
        } else if self.key.is_some()
            || self.iv.is_some()
            || self.padding.is_some()
            || self.ciphertext_encoding.is_some()
            || self.plaintext_encoding.is_some()
        {
            return Err(CryptoError::new(
                "none/base64/base64url do not accept AES key, iv, padding, or encoding fields",
            ));
        }
        if self.algorithm == Algorithm::None
            && self.base64_decode != Base64Decode::default()
        {
            return Err(CryptoError::new(
                "none does not use base64_decode options",
            ));
        }
        Ok(())
    }
}

impl DeserializeYaml for CryptoConfig {
    fn expected() -> Expected {
        Expected::Mapping
    }
    fn deserialize(
        yaml: SourcedYaml,
        source_map: &SourceMap,
    ) -> yaml::Result<Self> {
        let location = yaml.location;
        let mut d = StructDeserializer::new(yaml)?;
        let config = Self {
            algorithm: d.get(Field::new("algorithm"), source_map)?,
            key: d.get(Field::new("key").opt(), source_map)?,
            iv: d.get(Field::new("iv").opt(), source_map)?,
            padding: d.get(Field::new("padding").opt(), source_map)?,
            plaintext_encoding: d
                .get(Field::new("plaintext_encoding").opt(), source_map)?,
            ciphertext_encoding: d
                .get(Field::new("ciphertext_encoding").opt(), source_map)?,
            base64_decode: d
                .get(Field::new("base64_decode").opt(), source_map)?,
        };
        d.done()?;
        config
            .validate()
            .map_err(|e| LocatedError::other(e, location))?;
        Ok(config)
    }
}

/// Ordered, transactional transforms for an in-memory view of a response.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ResponseTransform {
    #[serde(rename = "type")]
    pub kind: TransformKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crypto: Option<String>,
    #[cfg_attr(feature = "schema", schemars(default))]
    pub scope: TransformScope,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub paths: Vec<String>,
    #[cfg_attr(feature = "schema", schemars(default))]
    pub skip_missing: bool,
    #[cfg_attr(feature = "schema", schemars(default))]
    pub skip_null: bool,
    #[cfg_attr(feature = "schema", schemars(default))]
    pub skip_blank: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse: Option<ParseAs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_encoding: Option<TextEncoding>,
}

impl ResponseTransform {
    fn validate(&self) -> Result<(), CryptoError> {
        if self.kind == TransformKind::ParseJson {
            if self.crypto.is_some() || self.scope != TransformScope::Fields {
                return Err(CryptoError::new(
                    "parse_json requires fields scope and no crypto",
                ));
            }
        } else if self.crypto.as_ref().is_none_or(String::is_empty) {
            return Err(CryptoError::new(
                "decode/decrypt requires a crypto name",
            ));
        }
        match self.scope {
            TransformScope::Body => {
                if self.parse != Some(ParseAs::Json)
                    || !self.paths.is_empty()
                    || self.skip_missing
                    || self.skip_null
                    || self.skip_blank
                {
                    return Err(CryptoError::new(
                        "body scope requires parse: json, no paths, and no field skip flags",
                    ));
                }
            }
            TransformScope::Fields => {
                if self.paths.is_empty()
                    || self.parse.is_some()
                    || self.text_encoding.is_some()
                {
                    return Err(CryptoError::new(
                        "fields scope requires paths and no body parse/text_encoding options",
                    ));
                }
                for path in &self.paths {
                    FieldPath::parse(path)?;
                }
            }
        }
        Ok(())
    }
}

impl DeserializeYaml for ResponseTransform {
    fn expected() -> Expected {
        Expected::Mapping
    }
    fn deserialize(
        yaml: SourcedYaml,
        source_map: &SourceMap,
    ) -> yaml::Result<Self> {
        let location = yaml.location;
        let mut d = StructDeserializer::new(yaml)?;
        let rule = Self {
            kind: d.get(Field::new("type"), source_map)?,
            crypto: d.get(Field::new("crypto").opt(), source_map)?,
            scope: d.get(
                Field::new("scope").or(TransformScope::Fields),
                source_map,
            )?,
            paths: d.get(Field::new("paths").opt(), source_map)?,
            skip_missing: d
                .get(Field::new("skip_missing").opt(), source_map)?,
            skip_null: d.get(Field::new("skip_null").opt(), source_map)?,
            skip_blank: d.get(Field::new("skip_blank").opt(), source_map)?,
            parse: d.get(Field::new("parse").opt(), source_map)?,
            text_encoding: d
                .get(Field::new("text_encoding").opt(), source_map)?,
        };
        d.done()?;
        rule.validate()
            .map_err(|e| LocatedError::other(e, location))?;
        Ok(rule)
    }
}
