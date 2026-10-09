use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn prepared_module_installs_without_a_job_and_obeys_guest_writes_and_remaps() {
    let engine = wasm86_test_support::engine().clone();
    let artifact_engine = engine.clone();
    // Model an artifact producer independent of this Runtime's worker. Even
    // test preparation keeps generation and engine compilation off this thread.
    let prepared = thread::spawn(move || {
        worker::compile(
            &artifact_engine,
            Request {
                profile: Profile::Flat32,
                entry: Entry::Block {
                    eip: 0x1000,
                    code: vec![0x40],
                    instruction_limit: 1,
                },
            },
        )
        .unwrap()
    })
    .join()
    .unwrap();
    let worker = Worker::spawn(move |request| {
        assert!(
            matches!(request.entry, Entry::Interpreter),
            "prepared code must not submit a block job"
        );
        worker::compile(&engine, request)
    })
    .unwrap();
    let mut machine = Machine::new(&[0x40, 0xeb, 0xfd], Some(worker));
    machine.remap(
        3,
        Mapping::Ram {
            backing: 0x1000,
            writable: true,
        },
    );
    // A separate guest entry changes INC to DEC through the writable alias.
    machine.write(0x2000, &[0xc6, 0x05, 0x00, 0x30, 0, 0, 0x48]);
    let ranges = [CodeRange {
        offset: 0x1000,
        bytes: 1,
    }];
    let ticket = machine.runtime.register_code(0x1000, &ranges).unwrap();
    assert!(machine.watched(3));
    assert!(machine.runtime.install(ticket, prepared.clone()).unwrap());
    assert!(!machine.runtime.install(ticket, prepared.clone()).unwrap());
    assert!(machine.runtime.pending.is_empty());

    let failed = machine.runtime.register_code(0x1000, &ranges).unwrap();
    assert!(machine
        .runtime
        .install(
            failed,
            CompiledEntry {
                entry: "missing".into(),
                ..prepared.clone()
            }
        )
        .is_err());
    assert!(!machine.runtime.install(failed, prepared.clone()).unwrap());
    assert!(machine.watched(3));

    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    machine.runtime.store_mut().call_hook(move |_, hook| {
        if matches!(hook, wasmtime::CallHook::CallingWasm) {
            counted.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    });
    assert_eq!(
        machine.runtime.run_slice(2).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, 1);
    // The failed replacement retains the old one-instruction block, which
    // returns before the interpreter's JMP.
    assert_eq!(calls.load(Ordering::Relaxed), 2);

    let mut cpu = machine.cpu();
    cpu.eip = 0x2000;
    machine.set_cpu(cpu);
    assert_eq!(
        machine.runtime.run_slice(1).unwrap().exit,
        SliceExit::Yielded
    );
    assert!(!machine.watched(1));
    assert!(!machine.runtime.install(ticket, prepared.clone()).unwrap());
    let mut cpu = machine.cpu();
    cpu.eip = 0x1000;
    machine.set_cpu(cpu);
    calls.store(0, Ordering::Relaxed);
    assert_eq!(
        machine.runtime.run_slice(2).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, 0);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    machine.write(0x1000, &[0x40]);
    let stale = machine.runtime.register_code(0x1000, &ranges).unwrap();
    machine.remap(
        1,
        Mapping::Ram {
            backing: 0x4000,
            writable: true,
        },
    );
    machine.remap(
        1,
        Mapping::Ram {
            backing: 0x1000,
            writable: true,
        },
    );
    assert!(!machine.runtime.install(stale, prepared.clone()).unwrap());
    let abandoned = machine.runtime.register_code(0x1000, &ranges).unwrap();
    machine.runtime.cancel_code(abandoned);
    assert!(!machine.watched(3));
    let ticket = machine.runtime.register_code(0x1000, &ranges).unwrap();
    assert!(machine.runtime.install(ticket, prepared).unwrap());
    machine.remap(1, Mapping::Unmapped);
    assert_eq!(
        machine.runtime.run_slice(1).unwrap().exit,
        SliceExit::Guest((4 << 48) | (16 << 32) | 0x1000)
    );
    assert_eq!(machine.cpu().instruction_count, 5);
}
