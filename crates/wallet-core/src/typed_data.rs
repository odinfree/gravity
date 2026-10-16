//! The pinned reference encoder treats selectors as function names (SNIP-12).
//! starknet.js also accepts pre-hashed hex selectors and hashes those documents
//! differently. Reject ambiguous values before signing; do not rewrite messages.
use crate::{CoreError, Result};
use serde_json::Value;

pub(crate) fn validate_selector_names(document: &str) -> Result<()> {
    let doc: Value = serde_json::from_str(document).map_err(|_| CoreError::Signing)?;
    // Gravity's signing/hash API advertises revision 1. The dependency also
    // supports revision 0; accepting it here would mislabel the returned hash.
    let revision = &doc["domain"]["revision"];
    if revision.as_str() != Some("1") && revision.as_u64() != Some(1) {
        return Err(CoreError::Signing);
    }
    let kind = doc["primaryType"].as_str().ok_or(CoreError::Signing)?;
    visit(&doc["types"], kind, &doc["message"], 0)
}

fn visit(types: &Value, kind: &str, value: &Value, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(CoreError::Signing);
    }
    if let Some(inner) = kind.strip_suffix('*') {
        for item in value.as_array().ok_or(CoreError::Signing)? {
            visit(types, inner, item, depth + 1)?;
        }
    } else if kind == "selector" {
        let name = value.as_str().ok_or(CoreError::Signing)?;
        let mut bytes = name.bytes();
        if !bytes
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            || !bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err(CoreError::Signing);
        }
    } else if let Some(fields) = types.get(kind).and_then(Value::as_array) {
        // Inline references (array elements, tuple entries, Merkle leaves and
        // primaryType) name an enum directly instead of using a field's
        // `type: "enum", contains: ...` wrapper. The reference parser has
        // already checked that a definition contains only fields or variants.
        if fields
            .first()
            .and_then(|field| field["type"].as_str())
            .is_some_and(|kind| kind.starts_with('('))
        {
            return visit_enum(types, fields, value, depth);
        }
        for field in fields {
            let name = field["name"].as_str().ok_or(CoreError::Signing)?;
            let kind = field["type"].as_str().ok_or(CoreError::Signing)?;
            let value = value.get(name).ok_or(CoreError::Signing)?;
            if kind == "enum" {
                let name = field["contains"].as_str().ok_or(CoreError::Signing)?;
                visit(types, name, value, depth + 1)?;
            } else if kind == "merkletree" {
                let leaf = field["contains"].as_str().ok_or(CoreError::Signing)?;
                for item in value.as_array().ok_or(CoreError::Signing)? {
                    visit(types, leaf, item, depth + 1)?;
                }
            } else {
                visit(types, kind, value, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn visit_enum(types: &Value, variants: &[Value], value: &Value, depth: usize) -> Result<()> {
    let selected = value.as_object().ok_or(CoreError::Signing)?;
    if selected.len() != 1 {
        return Err(CoreError::Signing);
    }
    let (name, payload) = selected.iter().next().ok_or(CoreError::Signing)?;
    let variant = variants
        .iter()
        .find(|variant| variant["name"] == *name)
        .ok_or(CoreError::Signing)?;
    let tuple = variant["type"].as_str().ok_or(CoreError::Signing)?;
    let tuple = tuple
        .strip_prefix('(')
        .and_then(|tuple| tuple.strip_suffix(')'))
        .ok_or(CoreError::Signing)?;
    let kinds: Vec<_> = if tuple.is_empty() {
        vec![]
    } else {
        tuple.split(',').collect()
    };
    let values = payload.as_array().ok_or(CoreError::Signing)?;
    if kinds.len() != values.len() {
        return Err(CoreError::Signing);
    }
    for (kind, value) in kinds.into_iter().zip(values) {
        visit(types, kind.trim(), value, depth + 1)?;
    }
    Ok(())
}
