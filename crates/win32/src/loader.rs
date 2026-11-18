use crate::{BuiltinDll, DllExport, DllSymbol, Exception, Exports, GuestMemory};
use relocade_machine::Cpu;
use std::collections::{btree_map::Entry, BTreeMap};
use std::fmt;
use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

/// Callable guest addresses in one loaded module.
#[derive(Debug)]
pub struct LoadedDll {
    symbols: BTreeMap<DllSymbol<'static>, u32>,
}

impl LoadedDll {
    /// Looks up an exact export name or a public ordinal, as for `GetProcAddress`.
    pub fn resolve(&self, symbol: DllSymbol<'_>) -> Option<u32> {
        self.symbols.get(&symbol).copied()
    }
}

/// Built-in modules loaded into a single guest process.
pub struct DllLoader {
    memory: Rc<dyn GuestMemory>,
    deliver_exception: Rc<dyn Fn(Exception)>,
    free_stubs: Range<u64>,
    modules: BTreeMap<String, LoadedDll>,
}

impl DllLoader {
    /// Reserves a caller-mapped guest range for export stubs. Keep this range
    /// private to the loader. A failed hook installation can leave stub bytes
    /// there, and the next load reuses the space. Guest address zero denotes NULL.
    pub fn new(
        memory: Rc<dyn GuestMemory>,
        stubs: RangeInclusive<u32>,
        deliver_exception: impl Fn(Exception) + 'static,
    ) -> Self {
        Self {
            memory,
            deliver_exception: Rc::new(deliver_exception),
            free_stubs: u64::from((*stubs.start()).max(1))..u64::from(*stubs.end()) + 1,
            modules: BTreeMap::new(),
        }
    }

    /// Finds a loaded DLL by its ASCII case-insensitive file name.
    pub fn module(&self, name: &str) -> Option<&LoadedDll> {
        self.modules.get(&name.to_ascii_lowercase())
    }

    /// Writes the resolved PE32 address to an import address table slot. A
    /// missing DLL or export returns `Ok(None)` and preserves the slot.
    pub fn bind_import(
        &self,
        module: &str,
        symbol: DllSymbol<'_>,
        slot: u32,
    ) -> Result<Option<u32>, Exception> {
        let Some(address) = self.module(module).and_then(|dll| dll.resolve(symbol)) else {
            return Ok(None);
        };
        self.memory.write(slot, &address.to_le_bytes())?;
        Ok(Some(address))
    }

    /// Loads a definition on the process's stopped Flat32 CPU, installing its
    /// typed functions as one batch. A repeated load returns the original module.
    pub async fn load<C: ?Sized + 'static>(
        &mut self,
        cpu: &mut Cpu,
        dll: &BuiltinDll<C>,
        context: Rc<C>,
    ) -> Result<&LoadedDll, DllLoadError> {
        let deliver = self.deliver_exception.clone();
        let mut bindings = Exports::new(cpu, self.memory.clone(), move |fault| deliver(fault));
        self.load_with(dll, async move |functions| {
            for (address, export) in functions {
                (export.hook)(&mut bindings, address, context.clone());
            }
            bindings.install().await
        })
        .await
    }

    async fn load_with<C: ?Sized + 'static>(
        &mut self,
        dll: &BuiltinDll<C>,
        install: impl AsyncFnOnce(Vec<(u32, &'static DllExport<C>)>) -> relocade_machine::Result<()>,
    ) -> Result<&LoadedDll, DllLoadError> {
        let vacant = match self.modules.entry(dll.name.to_ascii_lowercase()) {
            Entry::Occupied(entry) => return Ok(entry.into_mut()),
            Entry::Vacant(entry) => entry,
        };
        let mut cursor = self.free_stubs.start;
        let mut symbols = BTreeMap::new();
        let mut functions = Vec::with_capacity(dll.exports.len());
        let mut bytes = Vec::new();
        for export in dll.exports {
            let end = cursor + export.stub.len() as u64;
            if end > self.free_stubs.end {
                return Err(DllLoadError::StubSpaceExhausted);
            }
            let address = cursor as u32;
            for symbol in export.symbols() {
                if symbols.insert(symbol, address).is_some() {
                    return Err(DllLoadError::DuplicateSymbol(symbol));
                }
            }
            functions.push((address, export));
            bytes.extend_from_slice(export.stub);
            cursor = end;
        }
        if let Some(&(first, _)) = functions.first() {
            self.memory
                .write(first, &bytes)
                .map_err(DllLoadError::Memory)?;
            install(functions).await.map_err(DllLoadError::Install)?;
        }
        self.free_stubs.start = cursor;
        Ok(vacant.insert(LoadedDll { symbols }))
    }
}

/// Failure to publish a built-in module in a guest process.
#[derive(Debug)]
pub enum DllLoadError {
    DuplicateSymbol(DllSymbol<'static>),
    StubSpaceExhausted,
    Memory(Exception),
    Install(relocade_machine::Error),
}

impl fmt::Display for DllLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSymbol(symbol) => write!(f, "duplicate DLL export {symbol:?}"),
            Self::StubSpaceExhausted => f.write_str("insufficient guest space for DLL stubs"),
            Self::Memory(fault) => fault.fmt(f),
            Self::Install(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DllLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Memory(fault) => Some(fault),
            Self::Install(error) => Some(error),
            _ => None,
        }
    }
}
