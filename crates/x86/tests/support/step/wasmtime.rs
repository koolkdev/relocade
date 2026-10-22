//! Wasmtime instantiation, host callbacks, and boundary observations.

use ::wasmtime::{Caller, Linker, Memory, MemoryType, Store, Trap};
use relocade_test_support::SharedBytes;
use std::sync::Arc;

use super::{
    changes, Argument, Event, Input, MmioUpdate, Observation, Outcome, SegmentQuery,
    SegmentResolution, Snapshot, TestModule,
};

mod physical;
mod ports;

struct ExecutionEvents {
    events: Vec<Event>,
    machine_unchanged: bool,
    segment_resolutions: std::vec::IntoIter<SegmentResolution>,
    segment_queries: std::vec::IntoIter<SegmentQuery>,
    mmio_updates: std::vec::IntoIter<MmioUpdate>,
    port_reads: std::vec::IntoIter<u32>,
}

impl TestModule {
    pub(crate) fn observe(&self, input: &Input, invocations: usize) -> Observation {
        let engine = relocade_test_support::engine();
        let module = self.module.wasmtime();
        let mut store = Store::new(
            engine,
            ExecutionEvents {
                events: Vec::new(),
                machine_unchanged: true,
                segment_resolutions: input.segment_resolutions.clone().into_iter(),
                segment_queries: input.segment_queries.clone().into_iter(),
                mmio_updates: input.mmio_updates.clone().into_iter(),
                port_reads: input.port_reads.clone().into_iter(),
            },
        );
        let cpu = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
        let guest = SharedBytes::new(1, 1);
        let machine = SharedBytes::new(64, 64);
        let context = crate::ExecutionContext {
            cpu_model: self
                .target
                .map(|target| target.cpu_model)
                .unwrap_or_default(),
            ..crate::ExecutionContext::default()
        };
        cpu.write(&mut store, 0, &context.to_bytes()).unwrap();
        cpu.write(&mut store, 0, &input.cpu).unwrap();
        for (offset, bytes) in &input.guest {
            guest.write(*offset, bytes);
        }
        for (offset, bytes) in &input.machine {
            machine.write(*offset, bytes);
        }
        let guest_before: Arc<[u8]> = guest.read(0, 65536).into();
        // CPU-only observers without host mapping edits cannot change machine
        // memory. Avoid its four-megabyte copies at flag-observation checkpoints.
        let observes_machine = self.interpreter.is_some()
            || module
                .imports()
                .any(|import| import.module() == "relocade" && import.name() == "machine")
            || input
                .patches_before_calls
                .iter()
                .any(|patches| !patches.machine.is_empty());
        let machine_before =
            observes_machine.then(|| Arc::<[u8]>::from(machine.read(0, 64 * 65536)));
        let mut linker = Linker::new(engine);
        ports::register(&mut linker);
        linker
            .define(&store, "relocade", "executionContext", cpu)
            .unwrap();
        linker
            .define(&store, "relocade", "machine", machine.memory().clone())
            .unwrap();
        linker
            .define(&store, "relocade", "guest", guest.memory().clone())
            .unwrap();
        let cpu_len = input.cpu.len();
        let observe_guest = input.observe_guest;
        let dispatch_return = input.dispatch_return;
        let dispatch_guest_before = guest_before.clone();
        let dispatch_machine_before = machine_before.clone();
        let dispatch_guest = guest.clone();
        let dispatch_machine = machine.clone();
        linker
            .func_wrap(
                "relocade",
                "dispatch",
                move |mut caller: Caller<'_, ExecutionEvents>| {
                    let snapshot = Snapshot {
                        cpu: cpu.data(&caller)[..cpu_len].to_vec(),
                        guest: observe_guest.then(|| {
                            changes(&dispatch_guest_before, &dispatch_guest.read(0, 65536))
                        }),
                    };
                    let unchanged = dispatch_machine_before
                        .as_ref()
                        .is_none_or(|before| &**before == dispatch_machine.read(0, 64 * 65536));
                    caller.data_mut().machine_unchanged &= unchanged;
                    caller.data_mut().events.push(Event::Dispatch { snapshot });
                    dispatch_return
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "relocade",
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
                    let [status, error, base, limit, selector, usable, readable, writable, executable, expand_down, big, needs_completion] =
                        reply.values.map(|value| value as i32);
                    (status, error, base, limit, selector, usable, readable, writable, executable, expand_down, big, needs_completion)
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "relocade",
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
                    let [lar_valid, lsl_valid, readable, writable, access_rights, limit] =
                        reply.values.map(|value| value as i32);
                    (
                        lar_valid,
                        lsl_valid,
                        readable,
                        writable,
                        access_rights,
                        limit,
                    )
                },
            )
            .unwrap();
        linker
            .func_wrap::<_, ()>("relocade", "completeSegmentLoad", |_: i32| {
                panic!("pre-accessed fixture descriptors require no completion");
            })
            .unwrap();
        if std::iter::once(module)
            .chain(
                self.interpreter
                    .map(|interpreter| interpreter.module.wasmtime()),
            )
            .any(|module| {
                module
                    .imports()
                    .any(|import| import.module() == "relocade" && import.name() == "physicalMap")
            })
        {
            physical::register(&mut linker, &mut store, guest.clone(), input);
        }
        if let Some(interpreter) = self.interpreter {
            let instance = linker
                .instantiate(&mut store, interpreter.module.wasmtime())
                .expect("instantiate the handoff interpreter");
            let entry = instance
                .get_func(&mut store, &interpreter.entry)
                .expect("the interpreter entry is exported");
            linker
                .define(&store, "relocade", "interpret", entry)
                .unwrap();
        } else {
            // This callback probes the restart boundary without executing it.
            let handoff_guest_before = guest_before.clone();
            let handoff_machine_before = machine_before.clone();
            let handoff_guest = guest.clone();
            let handoff_machine = machine.clone();
            linker
                .func_wrap(
                    "relocade",
                    "interpret",
                    move |mut caller: Caller<'_, ExecutionEvents>| {
                        let snapshot = Snapshot {
                            cpu: cpu.data(&caller)[..cpu_len].to_vec(),
                            guest: observe_guest.then(|| {
                                changes(&handoff_guest_before, &handoff_guest.read(0, 65536))
                            }),
                        };
                        let unchanged = handoff_machine_before
                            .as_ref()
                            .is_none_or(|before| &**before == handoff_machine.read(0, 64 * 65536));
                        caller.data_mut().machine_unchanged &= unchanged;
                        caller.data_mut().events.push(Event::Interpret { snapshot });
                        dispatch_return
                    },
                )
                .unwrap();
        }
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
                for (offset, bytes) in &patches.cpu {
                    cpu.write(&mut store, *offset as usize, bytes).unwrap();
                }
                for (offset, bytes) in &patches.machine {
                    machine.write(*offset, bytes);
                }
                for (offset, bytes) in &patches.guest {
                    guest.write(*offset, bytes);
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
                guest: observe_guest.then(|| changes(&guest_before, &guest.read(0, 65536))),
            };
            let unchanged = machine_before
                .as_ref()
                .is_none_or(|before| &**before == machine.read(0, 64 * 65536));
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
        assert_eq!(store.data().mmio_updates.len(), 0, "unused MMIO updates");
        assert_eq!(store.data().port_reads.len(), 0, "unused port reads");
        let guest_unchanged = &*guest_before == guest.read(0, 65536);
        let machine_unchanged = store.data().machine_unchanged;
        Observation {
            events: store.into_data().events,
            guest_unchanged,
            machine_unchanged,
        }
    }
}
