//! Shared rejection of control characters before parsing or persistence.
use crate::error::ApiError;
use serde_json::Value;

/// Allow ordinary text and tab/newline/carriage return, rejecting NUL and other C0 controls.
pub fn text(value: &str) -> Result<(), ApiError> {
    if value
        .chars()
        .any(|c| c < ' ' && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(ApiError::validation(
            "Text contains prohibited control characters",
        ));
    }
    Ok(())
}

/// Validate every JSON string, including object keys (such as tag names).
pub fn json(value: &Value) -> Result<(), ApiError> {
    match value {
        Value::String(value) => text(value),
        Value::Array(values) => values.iter().try_for_each(json),
        Value::Object(values) => values.iter().try_for_each(|(key, value)| {
            text(key)?;
            json(value)
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_controls_in_nested_text_and_keys() {
        for c in (0..32).filter(|c| !matches!(c, 9 | 10 | 13)) {
            assert!(text(&format!("a{}b", char::from(c))).is_err());
        }
        assert!(text("a\tb\nc\rλ").is_ok());
        assert!(json(&serde_json::json!({"tags":{"bad\u{0}":"value"}})).is_err());
        assert!(json(&serde_json::json!({"note":["bad\u{1}"]})).is_err());
    }
}
