//! Share interpreter code by its entry assumptions and memory backend across compiler configurations.

use super::TestModule;
use crate::support::config::FLAT32;
use crate::{build_interpreter, EntryAssumptions, MemoryBackend};
use relocade_test_support::Module;
use relocade_x86::CompilerConfig;
use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum InterpreterEntry {
    Step,
    Run,
}

impl TestModule {
    pub(crate) fn interpreter() -> &'static Self {
        Self::interpreter_with_config(FLAT32)
    }

    pub(crate) fn interpreter_with_config(config: CompilerConfig) -> &'static Self {
        Self::cached_interpreter(config, InterpreterEntry::Step)
    }

    pub(crate) fn interpreter_run_with_config(config: CompilerConfig) -> &'static Self {
        Self::cached_interpreter(config, InterpreterEntry::Run)
    }

    fn cached_interpreter(config: CompilerConfig, entry: InterpreterEntry) -> &'static Self {
        static CODE: Fixtures<(EntryAssumptions, MemoryBackend), Arc<Module>> = Fixtures::new();
        static MODULES: Fixtures<(CompilerConfig, InterpreterEntry), TestModule> = Fixtures::new();
        MODULES.get((config, entry), || {
            // The same physical PC interpreter runs canonical real mode and restored protected CPUs.
            let entry_assumptions = if config.entry == EntryAssumptions::REAL_MODE
                && config.memory_backend == MemoryBackend::PhysicalBus
            {
                EntryAssumptions::LIVE
            } else {
                config.entry
            };
            let module = CODE.get((entry_assumptions, config.memory_backend), || {
                if let Some(bytes) =
                    relocade_interpreter::module(entry_assumptions, config.memory_backend)
                {
                    Arc::new(Module::new(bytes))
                } else {
                    let program = build_interpreter(entry_assumptions, config.memory_backend)
                        .expect("build interpreter for the requested memory backend");
                    Arc::new(Module::new(&program.compile().unwrap()))
                }
            });
            Self {
                module: Arc::clone(module),
                entry: match entry {
                    InterpreterEntry::Step => crate::abi::STEP.name,
                    InterpreterEntry::Run => crate::abi::RUN.name,
                }
                .into(),
                config: Some(config),
                interpreter: None,
            }
        })
    }
}

/// Initialize each fixture once without serializing construction of different keys.
/// Fixtures live for the test process, including references held by linked modules.
struct Fixtures<K, V: 'static>(OnceLock<Mutex<HashMap<K, &'static OnceLock<V>>>>);

impl<K: Eq + Hash, V> Fixtures<K, V> {
    const fn new() -> Self {
        Self(OnceLock::new())
    }

    fn get(&'static self, key: K, build: impl FnOnce() -> V) -> &'static V {
        let cell = *self
            .0
            .get_or_init(Mutex::default)
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| Box::leak(Box::new(OnceLock::new())));
        cell.get_or_init(build)
    }
}
