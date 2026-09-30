use super::CryptoError;
use serde_json::Value;
use std::collections::BTreeSet;

/// A small writable JSONPath subset, resolving to canonical JSON pointers.
#[derive(Debug)]
pub(super) struct FieldPath(Vec<Segment>);
#[derive(Debug)]
enum Segment {
    Field(String),
    Index(usize),
    Wildcard,
}

impl FieldPath {
    pub fn parse(path: &str) -> Result<Self, CryptoError> {
        let bad = || {
            CryptoError::new(
                "invalid JSONPath; supported: $, .field, [index], [*], and [\"field\"]",
            )
        };
        let mut rest = path.strip_prefix('$').ok_or_else(bad)?;
        let mut segments = Vec::new();
        while !rest.is_empty() {
            if segments.len() >= 128 {
                return Err(CryptoError::new("JSONPath exceeds 128 segments"));
            }
            if let Some(tail) = rest.strip_prefix('.') {
                let end = tail.find(['.', '[']).unwrap_or(tail.len());
                let name = &tail[..end];
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(bad());
                }
                segments.push(Segment::Field(name.to_owned()));
                rest = &tail[end..];
            } else if let Some(tail) = rest.strip_prefix('[') {
                if tail.starts_with('"') {
                    let mut escaped = false;
                    let mut close = None;
                    for (index, ch) in tail.char_indices().skip(1) {
                        if escaped {
                            escaped = false;
                        } else if ch == '\\' {
                            escaped = true;
                        } else if ch == '"' {
                            close = Some(index);
                            break;
                        }
                    }
                    let close = close.ok_or_else(bad)?;
                    let field = serde_json::from_str::<String>(&tail[..=close])
                        .map_err(|_| bad())?;
                    rest =
                        tail[close + 1..].strip_prefix(']').ok_or_else(bad)?;
                    segments.push(Segment::Field(field));
                } else {
                    let end = tail.find(']').ok_or_else(bad)?;
                    let subscript = &tail[..end];
                    let segment = if subscript == "*" {
                        Segment::Wildcard
                    } else if !subscript.is_empty()
                        && subscript.bytes().all(|b| b.is_ascii_digit())
                    {
                        Segment::Index(subscript.parse().map_err(|_| bad())?)
                    } else {
                        return Err(bad());
                    };
                    segments.push(segment);
                    rest = &tail[end + 1..];
                }
            } else {
                return Err(bad());
            }
        }
        Ok(Self(segments))
    }

    pub fn select(
        &self,
        root: &Value,
        skip_missing: bool,
    ) -> Result<BTreeSet<String>, CryptoError> {
        fn walk(
            value: &Value,
            segments: &[Segment],
            pointer: String,
            skip: bool,
            found: &mut BTreeSet<String>,
        ) -> Result<(), CryptoError> {
            let Some((segment, tail)) = segments.split_first() else {
                found.insert(pointer);
                return Ok(());
            };
            let missing = || {
                if skip {
                    Ok(())
                } else {
                    Err(CryptoError::new("JSONPath did not find a value"))
                }
            };
            match segment {
                Segment::Field(key) => {
                    let object = value.as_object().ok_or_else(|| CryptoError::new("JSONPath expected an object (use explicit parse_json for JSON text)"))?;
                    if let Some(value) = object.get(key) {
                        let key = key.replace('~', "~0").replace('/', "~1");
                        walk(
                            value,
                            tail,
                            format!("{pointer}/{key}"),
                            skip,
                            found,
                        )
                    } else {
                        missing()
                    }
                }
                Segment::Index(index) => {
                    let array = value.as_array().ok_or_else(|| {
                        CryptoError::new("JSONPath expected an array")
                    })?;
                    if let Some(value) = array.get(*index) {
                        walk(
                            value,
                            tail,
                            format!("{pointer}/{index}"),
                            skip,
                            found,
                        )
                    } else {
                        missing()
                    }
                }
                Segment::Wildcard => {
                    let array = value.as_array().ok_or_else(|| CryptoError::new("JSONPath [*] expected an array; strings are not implicitly parsed"))?;
                    if array.is_empty() {
                        return missing();
                    }
                    for (index, value) in array.iter().enumerate() {
                        walk(
                            value,
                            tail,
                            format!("{pointer}/{index}"),
                            skip,
                            found,
                        )?;
                    }
                    Ok(())
                }
            }
        }
        let mut found = BTreeSet::new();
        walk(root, &self.0, String::new(), skip_missing, &mut found)?;
        Ok(found)
    }
}
