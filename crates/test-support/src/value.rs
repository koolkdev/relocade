use serde::{Deserialize, Serialize};
use wasmtime::Val;

/// Wasm carrier values at the host boundary, independent of logical guest types.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum Value {
    I32(i32),
    I64(#[serde(with = "decimal_i64")] i64),
}

impl Value {
    pub fn wasm(self) -> Val {
        match self {
            Self::I32(value) => Val::I32(value),
            Self::I64(value) => Val::I64(value),
        }
    }

    pub fn from_wasm(value: &Val) -> Self {
        match value {
            Val::I32(value) => Self::I32(*value),
            Val::I64(value) => Self::I64(*value),
            _ => panic!("test host values must be integer carriers"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Outcome {
    Returned(Option<Value>),
    Trap,
}

/// Preserve i64 fields across JSON and JavaScript's Number boundary. V8 host
/// adapters convert these decimal strings with BigInt.
pub mod decimal_i64 {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{Outcome, Value};

    #[test]
    fn wire_values_preserve_integer_limits_and_void_returns() {
        for (value, json) in [
            (
                Value::I32(i32::MIN),
                r#"{"type":"i32","value":-2147483648}"#,
            ),
            (
                Value::I64(i64::MIN),
                r#"{"type":"i64","value":"-9223372036854775808"}"#,
            ),
            (
                Value::I64(i64::MAX),
                r#"{"type":"i64","value":"9223372036854775807"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(serde_json::from_str::<Value>(json).unwrap(), value);
        }
        assert_eq!(
            serde_json::to_string(&Outcome::Returned(None)).unwrap(),
            r#"{"kind":"returned","value":null}"#
        );
        assert_eq!(
            serde_json::from_str::<Outcome>(r#"{"kind":"trap"}"#).unwrap(),
            Outcome::Trap
        );
    }
}
