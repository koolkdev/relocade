use super::WasmBackend;
use wasm86_compiler::{abi, I64};

/// A binding whose name and type come from a shared ABI declaration.
pub struct Import<'a, B: WasmBackend> {
    module: &'a str,
    name: &'a str,
    value: ImportValue<'a, B>,
}

impl<B: WasmBackend> Clone for Import<'_, B> {
    fn clone(&self) -> Self {
        Self {
            module: self.module,
            name: self.name,
            value: match &self.value {
                ImportValue::Memory(memory) => ImportValue::Memory(*memory),
                ImportValue::Table(table) => ImportValue::Table(*table),
                ImportValue::Function(function) => ImportValue::Function(function.clone()),
            },
        }
    }
}

impl<B: WasmBackend> Import<'_, B> {
    pub fn module(&self) -> &str {
        self.module
    }

    pub fn name(&self) -> &str {
        self.name
    }

    /// The already-checked binding, for a backend to install in its engine.
    pub fn value(&self) -> &ImportValue<'_, B> {
        &self.value
    }
}

impl<'a, B: WasmBackend> Import<'a, B> {
    pub(super) fn function<S>(
        declaration: abi::Function<'a, S>,
        function: B::ImportFunction,
    ) -> Self {
        Self {
            module: declaration.module,
            name: declaration.name,
            value: ImportValue::Function(function),
        }
    }
}

/// Maps a declaration to its resource handle.
pub trait ImportBinding<'a, B: WasmBackend> {
    type Value;
    fn bind(self, backend: &B, value: Self::Value) -> Import<'a, B>;
}

macro_rules! resource {
    ($declaration:ty, $kind:ident) => {
        impl<'a, B: WasmBackend + 'a> ImportBinding<'a, B> for $declaration {
            type Value = &'a B::$kind;
            fn bind(self, _: &B, value: Self::Value) -> Import<'a, B> {
                Import {
                    module: self.module,
                    name: self.name,
                    value: ImportValue::$kind(value),
                }
            }
        }
    };
}

resource!(abi::Memory, Memory);
resource!(abi::Table<fn() -> I64>, Table);

impl<'a, B: WasmBackend + 'a> ImportBinding<'a, B> for abi::Function<'a, fn() -> I64> {
    type Value = &'a B::Function;
    fn bind(self, backend: &B, value: Self::Value) -> Import<'a, B> {
        Import::function(self, backend.import_function(value))
    }
}

/// Concrete bindings consumed by backend implementations. Consumers construct
/// bindings with [`WasmBackend::bind`] or [`WasmBackend::bind_host`]. Function
/// signatures have already been checked before entering this heterogeneous list.
#[doc(hidden)]
pub enum ImportValue<'a, B: WasmBackend> {
    Memory(&'a B::Memory),
    Table(&'a B::Table),
    Function(B::ImportFunction),
}
