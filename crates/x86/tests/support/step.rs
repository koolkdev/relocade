#[cfg(test)]
mod tests;

use std::{path::Path, sync::OnceLock};

use crate::SegmentProfile;
use serde::{Deserialize, Serialize};
pub(crate) use wasm86_test_support::{Outcome, Value as Argument};
use wasmtime::{Caller, Linker, Memory, MemoryType, Module, Store, Trap};

#[derive(Serialize)]
pub(crate) struct Input {
    pub(crate) cpu: Vec<u8>,
    pub(crate) guest: Vec<(u32, Vec<u8>)>,
    pub(crate) machine: Vec<(u32, Vec<u8>)>,
    pub(crate) arguments: Vec<Argument>,
    pub(crate) observe_guest: bool,
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

struct ExecutionEvents {
    events: Vec<Event>,
    machine_unchanged: bool,
}

pub(crate) struct TestModule {
    bytes: Vec<u8>,
    compiled: OnceLock<Module>,
    pub(crate) entry: String,
    profile: Option<SegmentProfile>,
}

impl TestModule {
    pub(crate) fn new(module: &crate::CompiledModule) -> Self {
        Self {
            bytes: module.bytes.clone(),
            compiled: OnceLock::new(),
            entry: module.entry.clone(),
            profile: module.segment_profile,
        }
    }

    pub(crate) fn interpreter() -> &'static Self {
        Self::interpreter_with_profile(SegmentProfile::Flat32)
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

    pub(crate) fn observe(&self, input: &Input, invocations: usize) -> Observation {
        self.check_profile(input, invocations);
        let engine = wasm86_test_support::engine();
        let module = self
            .compiled
            .get_or_init(|| Module::new(engine, &self.bytes).expect("compile the test module"));
        let mut store = Store::new(
            engine,
            ExecutionEvents {
                events: Vec::new(),
                machine_unchanged: true,
            },
        );
        let cpu = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
        let guest = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
        let machine = Memory::new(&mut store, MemoryType::new(64, None)).unwrap();
        cpu.write(&mut store, 0, &input.cpu).unwrap();
        for (memory, patches) in [(guest, &input.guest), (machine, &input.machine)] {
            for (offset, bytes) in patches {
                memory.write(&mut store, *offset as usize, bytes).unwrap();
            }
        }
        let guest_before = guest.data(&store).to_vec();
        let machine_before = machine.data(&store).to_vec();
        let mut linker = Linker::new(engine);
        for (name, memory) in [("cpuState", cpu), ("guest", guest), ("machine", machine)] {
            linker.define(&store, "wasm86", name, memory).unwrap();
        }
        let cpu_len = input.cpu.len();
        let observe_guest = input.observe_guest;
        let dispatch_return = input.dispatch_return;
        let dispatch_guest_before = guest_before.clone();
        let dispatch_machine_before = machine_before.clone();
        linker
            .func_wrap(
                "wasm86",
                "dispatch",
                move |mut caller: Caller<'_, ExecutionEvents>, eip: i32| {
                    let snapshot = Snapshot {
                        cpu: cpu.data(&caller)[..cpu_len].to_vec(),
                        guest: observe_guest
                            .then(|| changes(&dispatch_guest_before, guest.data(&caller))),
                    };
                    let unchanged = dispatch_machine_before == machine.data(&caller);
                    caller.data_mut().machine_unchanged &= unchanged;
                    caller
                        .data_mut()
                        .events
                        .push(Event::Dispatch { eip, snapshot });
                    dispatch_return
                },
            )
            .unwrap();
        let instance = linker
            .instantiate(&mut store, module)
            .expect("instantiate the test module");
        let entry = instance
            .get_func(&mut store, &self.entry)
            .expect("the test entry is exported");
        let arguments = input
            .arguments
            .iter()
            .map(|value| value.wasm())
            .collect::<Vec<_>>();
        let mut results = vec![wasmtime::Val::I32(0); entry.ty(&store).results().len()];
        for call in 0..invocations {
            for (offset, bytes) in input
                .cpu_patches_before_calls
                .get(call)
                .into_iter()
                .flatten()
            {
                cpu.write(&mut store, *offset as usize, bytes).unwrap();
            }
            let outcome = match entry.call(&mut store, &arguments, &mut results) {
                Ok(()) => Outcome::Returned(results.iter().map(Argument::from_wasm).collect()),
                Err(error) if error.downcast_ref::<Trap>().is_some() => Outcome::Trap,
                Err(error) => panic!("calling test entry {} failed: {error:#}", self.entry),
            };
            let snapshot = Snapshot {
                cpu: cpu.data(&store)[..cpu_len].to_vec(),
                guest: observe_guest.then(|| changes(&guest_before, guest.data(&store))),
            };
            let unchanged = machine_before == machine.data(&store);
            store.data_mut().machine_unchanged &= unchanged;
            store
                .data_mut()
                .events
                .push(Event::Return { outcome, snapshot });
        }
        let guest_unchanged = guest_before == guest.data(&store);
        let machine_unchanged = store.data().machine_unchanged;
        Observation {
            events: store.into_data().events,
            guest_unchanged,
            machine_unchanged,
        }
    }

    pub(crate) fn observe_v8(&self, input: &Input, invocations: usize) -> Observation {
        self.check_profile(input, invocations);
        #[derive(Serialize)]
        struct Request<'a> {
            entry: &'a str,
            invocations: usize,
            input: &'a Input,
        }
        wasm86_test_support::run_v8(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/execute-step.mjs"),
            &self.bytes,
            &Request {
                entry: &self.entry,
                invocations,
                input,
            },
        )
    }

    fn check_profile(&self, input: &Input, invocations: usize) {
        let Some(profile) = self.profile else {
            return;
        };
        let mut bytes: [u8; crate::CpuState::BYTE_LEN] = input.cpu[..crate::CpuState::BYTE_LEN]
            .try_into()
            .expect("execution inputs contain the CPU image");
        let check = |bytes| {
            let cpu = crate::CpuState::from_bytes(bytes);
            assert!(
                profile.is_compatible_with(&cpu.segments),
                "{} requires compatible {profile:?} segment state",
                self.entry
            );
        };
        // Current instructions preserve segment caches. Explicit patches are the
        // only way these fixtures can change the profile between invocations.
        for call in 0..invocations {
            if let Some(patches) = input.cpu_patches_before_calls.get(call) {
                for (offset, patch) in patches {
                    let start = *offset as usize;
                    if start < bytes.len() {
                        let count = patch.len().min(bytes.len() - start);
                        bytes[start..start + count].copy_from_slice(&patch[..count]);
                    }
                }
            }
            check(bytes);
        }
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
