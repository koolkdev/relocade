use super::*;
use wasm86_x86::CpuState;

mod installation;
mod physical;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct Machine<T = ()> {
    runtime: Runtime<T>,
}

impl Machine {
    fn new(code: &[u8], worker: Option<Worker>) -> Self {
        let engine = wasm86_test_support::engine();
        let mut store = Store::new(engine, HostState::new(()));
        let cpu = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
        let guest = Memory::new(&mut store, MemoryType::new(1, None)).unwrap();
        let page_table = Memory::new(&mut store, MemoryType::new(64, None)).unwrap();
        let state = CpuState {
            eip: 0x1000,
            ..CpuState::default()
        };
        cpu.write(&mut store, 0, &state.to_bytes()).unwrap();
        guest.write(&mut store, 0x1000, code).unwrap();
        for page in 1..16u32 {
            page_table
                .write(
                    &mut store,
                    page as usize * 4,
                    &((page << 12) | 3).to_le_bytes(),
                )
                .unwrap();
        }
        let mut linker = Linker::new(engine);
        linker
            .func_wrap(
                "wasm86",
                "resolveSegment",
                |_: i32, _: i32| -> (i32, i32, i32, i32, i32, i32) {
                    panic!("unexpected segment load")
                },
            )
            .unwrap();
        linker
            .func_wrap(
                "wasm86",
                "querySegmentDescriptor",
                |_: i32| -> (i32, i32, i32, i32, i32) { panic!("unexpected descriptor query") },
            )
            .unwrap();
        let memory = HostMemory::new(&mut store, cpu, guest, page_table, Profile::Flat32);
        let runtime = match worker {
            Some(worker) => Runtime::with_worker(store, linker, memory, worker),
            None => Runtime::new(store, linker, memory),
        }
        .unwrap();
        let mut machine = Self { runtime };
        machine.wait_for(0, false);
        machine
    }
}

impl<T: 'static> Machine<T> {
    fn cpu(&self) -> CpuState {
        self.runtime.memory().read_cpu(self.runtime.store())
    }
    fn set_cpu(&mut self, cpu: CpuState) {
        self.runtime
            .memory()
            .write_cpu(self.runtime.store_mut(), &cpu)
            .unwrap();
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.runtime
            .memory()
            .write_backing(self.runtime.store_mut(), offset, bytes)
            .unwrap();
    }
    fn remap(&mut self, page: u32, mapping: Mapping) {
        self.runtime
            .memory()
            .remap(self.runtime.store_mut(), page, mapping);
    }
    fn watched(&self, page: usize) -> bool {
        let table = self.runtime.memory().mapping_memory();
        u32::from_le_bytes(
            table.data(self.runtime.store())[page * 4..page * 4 + 4]
                .try_into()
                .unwrap(),
        ) & wasm86_x86::CODE_WATCH
            != 0
    }
    fn wait_for(&mut self, id: u64, execute: bool) -> CompilationEvent {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let slice = self
                .runtime
                .run_slice(if execute { 10 } else { 0 })
                .unwrap();
            for event in slice.compilations {
                let event_id = match &event {
                    CompilationEvent::Installed { id }
                    | CompilationEvent::Discarded { id }
                    | CompilationEvent::Failed { id, .. } => *id,
                };
                if event_id == id {
                    return event;
                }
            }
            assert!(Instant::now() < deadline, "compilation did not finish");
            thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn guest_runs_while_generation_and_engine_compilation_are_blocked() {
    let engine = wasm86_test_support::engine().clone();
    let (started, starts) = mpsc::channel();
    let (release, releases) = mpsc::channel();
    let execution_thread = thread::current().id();
    let worker = Worker::spawn(move |request| {
        assert_ne!(thread::current().id(), execution_thread);
        if matches!(request.entry, Entry::Block { .. }) {
            started.send(()).unwrap();
            releases.recv().unwrap();
        }
        worker::compile(&engine, request)
    })
    .unwrap();
    let code = [0x40, 0xeb, 0xfd]; // INC EAX; JMP back
    let mut machine = Machine::new(&code, Some(worker));
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    machine.runtime.store_mut().call_hook(move |_, hook| {
        if matches!(hook, wasmtime::CallHook::CallingWasm) {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    });
    let id = machine.runtime.request_block(0x1000, 1).unwrap();
    starts.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        machine.runtime.run_slice(10).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, 5);
    assert_eq!(machine.cpu().instruction_count, 10);
    release.send(()).unwrap();
    assert!(matches!(
        machine.wait_for(id, true),
        CompilationEvent::Installed { .. }
    ));
    let before = machine.cpu();
    calls.store(0, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        machine.runtime.run_slice(10).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, before.registers.eax + 5);
    // The installed one-instruction block returns before each JMP. Ignoring the
    // cache would execute both instructions in five interpreter calls instead.
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 10);

    let stale = machine.runtime.request_block(0x1000, 2).unwrap();
    starts.recv_timeout(Duration::from_secs(5)).unwrap();
    // A replacement in flight must leave the installed entry usable.
    calls.store(0, std::sync::atomic::Ordering::Relaxed);
    machine.runtime.run_slice(10).unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 10);
    machine.write(0x1000, &[0x48]);
    for _ in 1..MAX_PENDING {
        machine.runtime.request_block(0x1000, 2).unwrap();
    }
    assert_eq!(
        machine.runtime.request_block(0x1000, 2),
        Err(SubmitError::Full)
    );
    machine
        .runtime
        .memory()
        .invalidate_all(machine.runtime.store_mut());
    release.send(()).unwrap();
    assert!(matches!(
        machine.wait_for(stale, true),
        CompilationEvent::Discarded { .. }
    ));
    // Drop must not join the paused compiler or execute pending generation here.
    drop(machine);
    let _ = release.send(());
}

#[test]
fn pending_snapshots_watch_aliases_and_reject_remap_aba_and_context_changes() {
    let mut machine = Machine::new(&[0x40, 0xeb, 0xfd], None);
    machine.remap(
        3,
        Mapping::Ram {
            backing: 0x1000,
            writable: true,
        },
    );
    let id = machine.runtime.request_block(0x1000, 2).unwrap();
    assert!(machine.watched(1));
    assert!(machine.watched(3));
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
    assert!(!machine.watched(1));
    assert!(!machine.watched(3));
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Discarded { .. }
    ));

    let id = machine.runtime.request_block(0x1000, 2).unwrap();
    let mut cpu = machine.cpu();
    cpu.segments.cs.selector ^= 8;
    machine.set_cpu(cpu);
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Discarded { .. }
    ));
    assert!(!machine.watched(3));
    assert_eq!(
        machine.runtime.run_slice(2).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, 1);
}

#[test]
fn rep_writing_its_prefix_redecodes_after_one_checked_element() {
    let mut machine = Machine::new(&[0xf3, 0xaa, 0x40], None);
    let mut cpu = machine.cpu();
    cpu.registers.eax = 0x90;
    cpu.registers.ecx = 3;
    cpu.registers.edi = 0x1000;
    machine.set_cpu(cpu);
    let id = machine.runtime.request_block(0x1000, 1).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Installed { .. }
    ));
    assert_eq!(
        machine.runtime.run_slice(1).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().eip, 0x1000);
    assert_eq!(machine.cpu().instruction_count, 0);
    assert_eq!(machine.cpu().registers.ecx, 2);
    assert_eq!(machine.cpu().registers.edi, 0x1001);
    assert!(!machine.watched(1));
    // The prefix is now NOP: resumption fetches it instead of replaying REP.
    assert_eq!(
        machine.runtime.run_slice(1).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().eip, 0x1001);
    assert_eq!(machine.cpu().instruction_count, 1);
    assert_eq!(machine.cpu().registers.ecx, 2);
    assert_eq!(machine.cpu().registers.edi, 0x1001);
}

#[test]
fn protected_stores_host_writes_and_remaps_invalidate_installed_code() {
    // MOV byte [0x1007], 0x48; INC EAX; JMP -3. The store changes INC to DEC.
    let code = [0xc6, 0x05, 0x07, 0x10, 0, 0, 0x48, 0x40, 0xeb, 0xfd];
    let mut machine = Machine::new(&code, None);
    let id = machine.runtime.request_block(0x1000, 3).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Installed { .. }
    ));
    assert_eq!(
        machine.runtime.run_slice(2).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, u32::MAX);
    assert_eq!(machine.cpu().instruction_count, 2);

    let id = machine.runtime.request_block(0x1007, 2).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Installed { .. }
    ));
    machine.write(0x1007, &[0x40]);
    let mut state = machine.cpu();
    state.eip = 0x1007;
    machine.set_cpu(state);
    machine.runtime.run_slice(1).unwrap();
    assert_eq!(machine.cpu().registers.eax, 0); // live INC, not stale DEC

    let mut state = machine.cpu();
    state.eip = 0x1007;
    machine.set_cpu(state);
    let id = machine.runtime.request_block(0x1007, 1).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Installed { .. }
    ));
    machine.remap(1, Mapping::Unmapped);
    assert_eq!(
        machine.runtime.run_slice(1).unwrap().exit,
        SliceExit::Guest((4 << 48) | (16 << 32) | 0x1007)
    );
    assert_eq!(machine.cpu().instruction_count, 3);
}

#[test]
fn rep_resumes_through_installation_and_compile_failure_is_not_a_guest_fault() {
    let code = [0xf3, 0xaa]; // REP STOSB
    let mut machine = Machine::new(&code, None);
    let mut cpu = machine.cpu();
    cpu.registers.ecx = 100;
    cpu.registers.edi = 0x2000;
    cpu.registers.eax = 0x5a;
    machine.set_cpu(cpu);
    assert_eq!(
        machine.runtime.run_slice(2).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.ecx, 98);
    let id = machine.runtime.request_block(0x1000, 1).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Installed { .. }
    ));
    for _ in 0..49 {
        assert_eq!(
            machine.runtime.run_slice(2).unwrap().exit,
            SliceExit::Yielded
        );
    }
    assert_eq!(machine.cpu().instruction_count, 1);
    assert_eq!(machine.cpu().eip, 0x1002);
    assert_eq!(machine.cpu().registers.ecx, 0);
    assert_eq!(
        &machine
            .runtime
            .memory()
            .guest_memory()
            .data(machine.runtime.store())[0x2000..0x2064],
        &[0x5a; 100]
    );
    machine.write(0x1002, &[0x0f, 0xff]);
    let id = machine.runtime.request_block(0x1002, 1).unwrap();
    assert!(matches!(
        machine.wait_for(id, false),
        CompilationEvent::Failed { .. }
    ));
    assert!(!machine.watched(1));
    machine.write(0x1002, &[0x40]);
    assert_eq!(
        machine.runtime.run_slice(1).unwrap().exit,
        SliceExit::Yielded
    );
    assert_eq!(machine.cpu().registers.eax, 0x5b);
}
