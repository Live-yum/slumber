use super::{
    Algorithm, Base64Decode, CryptoConfig, CryptoError, MaterialEncoding,
    Transport,
};
use aes::{
    Aes128, Aes192, Aes256,
    cipher::{
        BlockModeDecrypt, BlockModeEncrypt, KeyInit, KeyIvInit,
        block_padding::Pkcs7,
    },
};
use base64::{
    Engine,
    prelude::{BASE64_STANDARD, BASE64_URL_SAFE},
};
use std::fmt;
use zeroize::Zeroizing;

/// Material belongs to one render/profile, never to a global cache.
pub struct ResolvedCrypto {
    algorithm: Algorithm,
    key: Zeroizing<Vec<u8>>,
    iv: Zeroizing<Vec<u8>>,
    transport: Transport,
    decode_options: Base64Decode,
}

impl fmt::Debug for ResolvedCrypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedCrypto")
            .field("algorithm", &self.algorithm)
            .finish_non_exhaustive()
    }
}

impl ResolvedCrypto {
    pub fn new(
        config: &CryptoConfig,
        key: Zeroizing<Vec<u8>>,
        iv: Zeroizing<Vec<u8>>,
    ) -> Result<Self, CryptoError> {
        if let Some(len) = config.algorithm.key_len() {
            if key.len() != len {
                return Err(CryptoError::new(format!(
                    "key.value must decode to {len} bytes"
                )));
            }
            if config.algorithm.is_cbc() && iv.len() != 16 {
                return Err(CryptoError::new(
                    "iv.value must decode to 16 bytes",
                ));
            }
            if !config.algorithm.is_cbc() && !iv.is_empty() {
                return Err(CryptoError::new("ECB must not have an IV"));
            }
        }
        Ok(Self {
            algorithm: config.algorithm,
            key,
            iv,
            transport: config.ciphertext_encoding.unwrap_or(Transport::Base64),
            decode_options: config.base64_decode,
        })
    }

    pub fn require_aes(&self) -> Result<(), CryptoError> {
        if self.algorithm.key_len().is_none() {
            Err(CryptoError::new(
                "encrypt/decrypt require AES; use encode/decode for none or Base64",
            ))
        } else {
            Ok(())
        }
    }

    pub fn encode(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        macro_rules! cbc {
            ($aes:ty) => {
                cbc::Encryptor::<$aes>::new_from_slices(&self.key, &self.iv)
                    .map_err(|_| CryptoError::new("invalid AES key/IV length"))?
                    .encrypt_padded_vec::<Pkcs7>(plaintext)
            };
        }
        macro_rules! ecb {
            ($aes:ty) => {
                ecb::Encryptor::<$aes>::new_from_slice(&self.key)
                    .map_err(|_| CryptoError::new("invalid AES key length"))?
                    .encrypt_padded_vec::<Pkcs7>(plaintext)
            };
        }
        let ciphertext = match self.algorithm {
            Algorithm::None => return Ok(plaintext.to_vec()),
            Algorithm::Base64 => {
                return Ok(BASE64_STANDARD.encode(plaintext).into_bytes());
            }
            Algorithm::Base64Url => {
                return Ok(BASE64_URL_SAFE.encode(plaintext).into_bytes());
            }
            Algorithm::Aes128Cbc => cbc!(Aes128),
            Algorithm::Aes192Cbc => cbc!(Aes192),
            Algorithm::Aes256Cbc => cbc!(Aes256),
            Algorithm::Aes128Ecb => ecb!(Aes128),
            Algorithm::Aes192Ecb => ecb!(Aes192),
            Algorithm::Aes256Ecb => ecb!(Aes256),
        };
        Ok(match self.transport {
            Transport::Base64 => BASE64_STANDARD.encode(ciphertext),
            Transport::Base64Url => BASE64_URL_SAFE.encode(ciphertext),
        }
        .into_bytes())
    }

    pub fn decode(&self, encoded: &[u8]) -> Result<Vec<u8>, CryptoError> {
        match self.algorithm {
            Algorithm::None => return Ok(encoded.to_vec()),
            Algorithm::Base64 => {
                return self.decode_options.decode(encoded, Transport::Base64);
            }
            Algorithm::Base64Url => {
                return self
                    .decode_options
                    .decode(encoded, Transport::Base64Url);
            }
            _ => {}
        }
        let ciphertext = self.decode_options.decode(encoded, self.transport)?;
        if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
            return Err(CryptoError::new(
                "AES ciphertext must contain a positive multiple of 16 bytes",
            ));
        }
        macro_rules! cbc {
            ($aes:ty) => {
                cbc::Decryptor::<$aes>::new_from_slices(&self.key, &self.iv)
                    .map_err(|_| CryptoError::new("invalid AES key/IV length"))?
                    .decrypt_padded_vec::<Pkcs7>(&ciphertext)
            };
        }
        macro_rules! ecb {
            ($aes:ty) => {
                ecb::Decryptor::<$aes>::new_from_slice(&self.key)
                    .map_err(|_| CryptoError::new("invalid AES key length"))?
                    .decrypt_padded_vec::<Pkcs7>(&ciphertext)
            };
        }
        let plaintext = match self.algorithm {
            Algorithm::Aes128Cbc => cbc!(Aes128),
            Algorithm::Aes192Cbc => cbc!(Aes192),
            Algorithm::Aes256Cbc => cbc!(Aes256),
            Algorithm::Aes128Ecb => ecb!(Aes128),
            Algorithm::Aes192Ecb => ecb!(Aes192),
            Algorithm::Aes256Ecb => ecb!(Aes256),
            _ => unreachable!("non-AES algorithms returned above"),
        };
        plaintext.map_err(|_| CryptoError::new("invalid PKCS7 padding (CBC/ECB do not authenticate ciphertext)"))
    }
}

fn ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

impl Base64Decode {
    pub(crate) fn decode(
        self,
        input: &[u8],
        transport: Transport,
    ) -> Result<Vec<u8>, CryptoError> {
        let mut input: Vec<u8> = input
            .iter()
            .copied()
            .filter(|b| !(self.ignore_ascii_whitespace && ascii_whitespace(*b)))
            .map(|b| match (transport, b) {
                (Transport::Base64Url, b'-') => b'+',
                (Transport::Base64Url, b'_') => b'/',
                _ => b,
            })
            .collect();
        if self.allow_missing_padding {
            while !input.len().is_multiple_of(4) {
                input.push(b'=');
            }
        }
        BASE64_STANDARD.decode(input).map_err(|_| {
            CryptoError::new("invalid Base64 alphabet, length, or padding")
        })
    }
}

pub(crate) fn decode_material(
    value: &str,
    encoding: MaterialEncoding,
    options: Base64Decode,
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let bytes = match encoding {
        MaterialEncoding::Utf8 | MaterialEncoding::Text => {
            value.as_bytes().to_vec()
        }
        MaterialEncoding::Base64 => options
            .decode(value.as_bytes(), Transport::Base64)
            .map_err(|_| {
                CryptoError::new(
                    "key/iv.value is not valid Base64 (value redacted)",
                )
            })?,
        MaterialEncoding::Hex => {
            let compact: Zeroizing<Vec<u8>> = Zeroizing::new(
                value.bytes().filter(|b| !ascii_whitespace(*b)).collect(),
            );
            if !compact.len().is_multiple_of(2) {
                return Err(CryptoError::new(
                    "key/iv.value has invalid hex length (value redacted)",
                ));
            }
            compact.chunks_exact(2).map(|pair| {
                let digit = |b: u8| (b as char).to_digit(16).ok_or_else(|| CryptoError::new("key/iv.value contains invalid hex (value redacted)"));
                Ok((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
            }).collect::<Result<Vec<_>, CryptoError>>()?
        }
    };
    Ok(Zeroizing::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use slumber_util::yaml::deserialize_str;

    #[test]
    fn independent_dotnet_vectors() {
        let config: CryptoConfig = deserialize_str("algorithm: aes-128-cbc\nkey: {value: '0123456789abcdef'}\niv: {value: '0123456789abcdef'}").unwrap();
        let codec = ResolvedCrypto::new(
            &config,
            Zeroizing::new(b"0123456789abcdef".to_vec()),
            Zeroizing::new(b"0123456789abcdef".to_vec()),
        )
        .unwrap();
        for (plain, encrypted) in [
            ("", "7Uf+4FRcP6fdBw1EuG6Y2Q=="),
            ("13800138000", "UEI1Z8QQS25WGasdhDJA4g=="),
            ("测试人员", "zAboTQoxNn96i0Z/KPwKGQ=="),
            (
                "1234567890abcdef",
                "dbPloHkdKgBH2huMBKZlMkR/vZI3jKgPWz10cus9K10=",
            ),
        ] {
            assert_eq!(
                codec.encode(plain.as_bytes()).unwrap(),
                encrypted.as_bytes()
            );
            assert_eq!(
                codec.decode(encrypted.as_bytes()).unwrap(),
                plain.as_bytes()
            );
        }
        assert!(!format!("{config:?} {codec:?}").contains("0123456789abcdef"));
        assert!(codec.decode(b"YQ==").is_err());
        assert!(codec.decode(b"!").is_err());
    }
}
