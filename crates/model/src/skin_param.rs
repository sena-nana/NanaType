use serde::{Deserialize, Serialize};

/// A user's value for a skin-declared parameter, as stored in settings.
///
/// The skin's schema gives the value its meaning: colours are `#rrggbb(aa)`
/// strings and enum choices are option ids, so both arrive as `Text`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl ParamValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(v) => Some(v),
            _ => None,
        }
    }
}

/// Parameter, group, preset and option ids: `[a-z0-9_]{1,32}`.
pub fn valid_param_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
