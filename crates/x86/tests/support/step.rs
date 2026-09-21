#[cfg(test)]
mod tests;

use std::{path::Path, sync::OnceLock};

use crate::SegmentProfile;
use serde::{Deserialize, Serialize};
use wasm86_test_support::Module;
pub(crate) use wasm86_test_support::{Outcome, Value as Argument};

mod segments;
mod wasmtime;
pub(crate) use segments::{SegmentQuery, SegmentResolution};

#[derive(Serialize)]
pub(crate) struct Input {
    pub(crate) cpu: Vec<u8>,
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) machine: Vec<(u32, Vec<u8>)>,
    pub(crate) arguments: Vec<Argument>,
    pub(crate) observe_guest: bool,
    pub(crate) segment_resolutions: Vec<SegmentResolution>,
    pub(crate) segment_queries: Vec<SegmentQuery>,
    pub(crate) cpu_patches_before_calls: Vec<Vec<(u32, Vec<u8>)>>,
    #[serde(with = "wasm86_test_support::decimal_i64")]
    pub(crate) dispatch_return: i64,
}

impl Input {
    pub(crate) fn new(cpu: &[u8]) -> Self {
        Self {
            cpu: cpu.to_vec(),
            guest: Vec::new(),
            machine: Vec::new(),
            arguments: Vec::new(),
            observe_guest: false,
            segment_resolutions: Vec::new(),
            segment_queries: Vec::new(),
            cpu_patches_before_calls: Vec::new(),
            dispatch_return: i64::MIN,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(crate) struct Snapshot {
    pub(crate) cpu: Vec<u8>,
    pub(crate) guest: Option<Vec<(u32, u8)>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Event {
    QuerySegmentDescriptor {
        selector: i32,
    },
    ResolveSegment {
        segment: i32,
        selector: i32,
    },
    Dispatch {
        eip: i32,
        snapshot: Snapshot,
    },
    Return {
        outcome: Outcome,
        snapshot: Snapshot,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(crate) struct Observation {
    pub(crate) events: Vec<Event>,
    pub(crate) guest_unchanged: bool,
    /// True only if machine memory is unchanged at every dispatch and return.
    pub(crate) machine_unchanged: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Engine {
    Wasmtime,
    V8,
}

impl Engine {
    pub(crate) fn observe(
        self,
        module: &TestModule,
        input: &Input,
        invocations: usize,
    ) -> Observation {
        match self {
            Self::Wasmtime => module.observe(input, invocations),
            Self::V8 => module.observe_v8(input, invocations),
        }
    }
}

pub(crate) struct TestModule {
    module: Module,
    pub(crate) entry: String,
    profile: Option<SegmentProfile>,
}

impl TestModule {
    pub(crate) fn new(module: &crate::CompiledModule) -> Self {
        Self {
            module: Module::new(&module.bytes),
            entry: module.entry.clone(),
            profile: module.segment_profile,
        }
    }

    pub(crate) fn interpreter() -> &'static Self {
        Self::interpreter_with_profile(SegmentProfile::Flat32)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self.module.bytes()
    }

    pub(crate) fn interpreter_with_profile(profile: SegmentProfile) -> &'static Self {
        static FLAT: OnceLock<TestModule> = OnceLock::new();
        static SEGMENTED: OnceLock<TestModule> = OnceLock::new();
        static SEGMENTED16: OnceLock<TestModule> = OnceLock::new();
        let module = match profile {
            SegmentProfile::Flat32 => &FLAT,
            SegmentProfile::Segmented32 => &SEGMENTED,
            SegmentProfile::Segmented16 => &SEGMENTED16,
        };
        module.get_or_init(|| Self::new(&crate::compile_interpreter_step(profile).unwrap()))
    }

    pub(crate) fn observe_v8(&self, input: &Input, invocations: usize) -> Observation {
        #[derive(Serialize)]
        struct Request<'a> {
            entry: &'a str,
            profile: Option<&'static str>,
            invocations: usize,
            input: &'a Input,
        }
        self.module.run_v8(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/execute-step.mjs"),
            &Request {
                entry: &self.entry,
                profile: self.profile.map(|profile| match profile {
                    SegmentProfile::Flat32 => "flat32",
                    SegmentProfile::Segmented32 => "segmented32",
                    SegmentProfile::Segmented16 => "segmented16",
                }),
                invocations,
                input,
            },
        )
    }

    fn check_profile(&self, bytes: &[u8]) {
        let Some(profile) = self.profile else {
            return;
        };
        let cpu = crate::CpuState::from_bytes(
            bytes[..crate::CpuState::BYTE_LEN]
                .try_into()
                .expect("the CPU image"),
        );
        assert!(
            profile.is_compatible_with(&cpu.segments),
            "{} requires compatible {profile:?} segment state",
            self.entry
        );
    }
}

fn changes(before: &[u8], after: &[u8]) -> Vec<(u32, u8)> {
    before
        .iter()
        .zip(after)
        .enumerate()
        .filter(|(_, (old, new))| old != new)
        .map(|(offset, (_, &new))| (offset as u32, new))
        .collect()
}
