use wasmtime::Val;

/// Wasm carrier values at the host boundary, independent of logical guest types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value {
    I32(i32),
    I64(i64),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Returned(Vec<Value>),
    Trap,
}
