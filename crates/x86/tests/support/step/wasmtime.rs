//! Wasmtime instantiation, host callbacks, and boundary observations.

use ::wasmtime::{Caller, Linker, Memory, MemoryType, Store, Trap};
use std::sync::Arc;

use super::{
    changes, Argument, Event, Input, Observation, Outcome, SegmentQuery, SegmentResolution,
    Snapshot, TestModule,
};

struct ExecutionEvents {
    events: Vec<Event>,
    machine_unchanged: bool,
    segment_resolutions: std::vec::IntoIter<SegmentResolution>,
    segment_queries: std::vec::IntoIter<SegmentQuery>,
}

impl TestModule {
    pub(crate) fn observe(&self, input: &Input, invocations: usize) -> Observation {
        let engine = wasm86_test_support::engine();
        let module = self.module.wasmtime();
        let mut store = Store::new(
            engine,
            ExecutionEvents {
                events: Vec::new(),
                machine_unchanged: true,
                segment_resolutions: input.segment_resolutions.clone().into_iter(),
                segment_queries: input.segment_queries.clone().into_iter(),
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
        let guest_before: Arc<[u8]> = guest.data(&store).into();
        // CPU-only observers without host mapping edits cannot change machine
        // memory. Avoid its four-megabyte copies at flag-observation checkpoints.
        let observes_machine = module
            .imports()
            .any(|import| import.module() == "wasm86" && import.name() == "machine")
            || input
                .patches_before_calls
                .iter()
                .any(|patches| !patches.machine.is_empty());
        let machine_before = observes_machine.then(|| Arc::<[u8]>::from(machine.data(&store)));
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
                    let unchanged = dispatch_machine_before
                        .as_ref()
                        .is_none_or(|before| &**before == machine.data(&caller));
                    caller.data_mut().machine_unchanged &= unchanged;
                    caller
                        .data_mut()
                        .events
                        .push(Event::Dispatch { eip, snapshot });
                    dispatch_return
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "wasm86",
                "resolveSegment",
                |mut caller: Caller<'_, ExecutionEvents>, segment: i32, selector: i32| {
                    let state = caller.data_mut();
                    let reply = state
                        .segment_resolutions
                        .next()
                        .expect("unexpected segment load");
                    assert_eq!(
                        (segment as u32, selector as u32),
                        (reply.segment, u32::from(reply.selector))
                    );
                    state
                        .events
                        .push(Event::ResolveSegment { segment, selector });
                    let [status, error, base, limit, selector, attributes] =
                        reply.values.map(|value| value as i32);
                    (status, error, base, limit, selector, attributes)
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "wasm86",
                "querySegmentDescriptor",
                |mut caller: Caller<'_, ExecutionEvents>, selector: i32| {
                    let state = caller.data_mut();
                    let reply = state
                        .segment_queries
                        .next()
                        .expect("unexpected segment query");
                    assert_eq!(selector as u32, u32::from(reply.selector));
                    state
                        .events
                        .push(Event::QuerySegmentDescriptor { selector });
                    let [visible, readable, writable, access_rights, limit] =
                        reply.values.map(|value| value as i32);
                    (visible, readable, writable, access_rights, limit)
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
        let mut results = vec![::wasmtime::Val::I32(0); entry.ty(&store).results().len()];
        for call in 0..invocations {
            if let Some(patches) = input.patches_before_calls.get(call) {
                for (memory, edits) in [
                    (cpu, &patches.cpu),
                    (guest, &patches.guest),
                    (machine, &patches.machine),
                ] {
                    for (offset, bytes) in edits {
                        memory.write(&mut store, *offset as usize, bytes).unwrap();
                    }
                }
            }
            self.check_profile(cpu.data(&store));
            let outcome = match entry.call(&mut store, &arguments, &mut results) {
                Ok(()) => Outcome::Returned(results.iter().map(Argument::from_wasm).collect()),
                Err(error) if error.downcast_ref::<Trap>().is_some() => Outcome::Trap,
                Err(error) => panic!("calling test entry {} failed: {error:#}", self.entry),
            };
            let snapshot = Snapshot {
                cpu: cpu.data(&store)[..cpu_len].to_vec(),
                guest: observe_guest.then(|| changes(&guest_before, guest.data(&store))),
            };
            let unchanged = machine_before
                .as_ref()
                .is_none_or(|before| &**before == machine.data(&store));
            store.data_mut().machine_unchanged &= unchanged;
            store
                .data_mut()
                .events
                .push(Event::Return { outcome, snapshot });
        }
        assert_eq!(
            store.data().segment_resolutions.len(),
            0,
            "unused segment resolutions"
        );
        assert_eq!(
            store.data().segment_queries.len(),
            0,
            "unused segment queries"
        );
        let guest_unchanged = &*guest_before == guest.data(&store);
        let machine_unchanged = store.data().machine_unchanged;
        Observation {
            events: store.into_data().events,
            guest_unchanged,
            machine_unchanged,
        }
    }
}
