//! Share interpreter code by profile while retaining per-target execution fixtures.

use super::TestModule;
use crate::{Compiler, ExecutionProfile, ExecutionTarget, SegmentProfile};
use relocade_test_support::Module;
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
        Self::interpreter_with_profile(SegmentProfile::Flat32)
    }

    pub(crate) fn interpreter_with_profile(profile: impl Into<ExecutionProfile>) -> &'static Self {
        Self::interpreter_with_target(Compiler::new(profile).target())
    }

    pub(crate) fn interpreter_with_target(target: ExecutionTarget) -> &'static Self {
        Self::cached_interpreter(target, InterpreterEntry::Step)
    }

    pub(crate) fn interpreter_run_with_profile(
        profile: impl Into<ExecutionProfile>,
    ) -> &'static Self {
        Self::interpreter_run_with_target(Compiler::new(profile).target())
    }

    pub(crate) fn interpreter_run_with_target(target: ExecutionTarget) -> &'static Self {
        Self::cached_interpreter(target, InterpreterEntry::Run)
    }

    fn cached_interpreter(target: ExecutionTarget, entry: InterpreterEntry) -> &'static Self {
        static CODE: Fixtures<ExecutionProfile, Arc<Module>> = Fixtures::new();
        static TARGETS: Fixtures<(ExecutionTarget, InterpreterEntry), TestModule> = Fixtures::new();
        TARGETS.get((target, entry), || {
            let module = CODE.get(target.profile, || {
                Arc::new(Module::new(asset_bytes(target.profile)))
            });
            Self {
                module: Arc::clone(module),
                entry: match entry {
                    InterpreterEntry::Step => crate::abi::STEP.name,
                    InterpreterEntry::Run => crate::abi::RUN.name,
                }
                .into(),
                target: Some(target),
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

// Unit tests compile their own x86 types; the asset crate links the normal library.
// Translate the profile at that crate boundary without changing its assumptions.
pub(crate) fn asset_bytes(profile: ExecutionProfile) -> &'static [u8] {
    use relocade_interpreter::{ExecutionProfile as AssetProfile, SegmentProfile as AssetSegments};
    let profile = match profile {
        ExecutionProfile::Protected(segments) => AssetProfile::Protected(match segments {
            SegmentProfile::Flat32 => AssetSegments::Flat32,
            SegmentProfile::Segmented32 => AssetSegments::Segmented32,
            SegmentProfile::Segmented16 => AssetSegments::Segmented16,
        }),
        ExecutionProfile::Real16 => AssetProfile::Real16,
    };
    relocade_interpreter::module(profile)
}
