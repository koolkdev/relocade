use super::*;
use relocade_x86::{CodeAddress, Segment, SegmentRegister};

async fn requested_segments(service: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let first = FarPointer {
        segment: 0x10,
        offset: 0x10,
    };
    let alias = FarPointer {
        segment: 0x11,
        offset: 0,
    };
    let other = FarPointer {
        segment: 0x30,
        offset: 0x10,
    };
    assert_eq!(first.address(), alias.address());
    let entries = [other, first, alias];
    for (entry, immediate) in [(first, 42), (other, 73)] {
        machine
            .memory()
            .view()
            .write(entry.address(), &[0xb8, immediate, 0])
            .unwrap();
    }
    // MOV [0321], AL with DS=FFFF writes the last entry's immediate through A20.
    // Jump to the stop hook after the store.
    machine
        .memory()
        .view()
        .write(0x400, &[0xa2, 0x21, 0x03, 0xeb, 0x00])
        .unwrap();
    let cpu = machine.create_cpu(config, Some(service)).unwrap();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let state = runner.cpu().cpu_state();
        let mut compiler = runner.take_compiler().unwrap();
        compiler
            .request(
                state.compiler_config(),
                entries
                    .map(|entry| BlockRequest {
                        entry,
                        byte_len: 3,
                        instruction_limit: 1,
                    })
                    .to_vec(),
            )
            .unwrap();

        // The stopped CPU is in a fourth segment when the worker result arrives.
        state.set_segment(Segment::Cs, SegmentRegister::real_mode(Segment::Cs, 0x40));
        let compiled = compiler.recv().await.unwrap().unwrap();
        assert_eq!(
            compiled
                .blocks()
                .iter()
                .map(|block| block.address)
                .collect::<Vec<_>>(),
            entries.map(CodeAddress::from)
        );
        assert_eq!(compiled.ranges(), &[0x110..0x113, 0x310..0x313]);
        state.set_eip(0);
        state.set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0xffff));
        state.set_al(99);
        let stop = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x0040_0005, async move || stop.request())])
            .await
            .unwrap();
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        let memory = runner.cpu().memory();
        assert_eq!(memory.view().read_u8(0x311).unwrap(), 99);
        assert!(matches!(
            compiled.install(runner.cpu()),
            Err(relocade_machine::Error::StaleCompilation)
        ));
        assert!(memory.installed_code_at(0x1311).is_empty());
        assert_eq!(runner.cpu().compiled_modules_at(first.address()).count(), 0);
        memory.view().write_u8(0x311, 73).unwrap();
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install(entries.into_iter().flat_map(|entry| {
                let exit = exit.clone();
                [
                    Hook::host(entry.packed(), async || {
                        panic!("the compiled block must replace this CS:IP hook")
                    }),
                    Hook::host(entry.wrapping_add(3).packed(), async move || exit.request()),
                ]
            }))
            .await
            .unwrap();
        compiled.install(runner.cpu()).unwrap();

        for (entry, expected) in [(first, 42), (alias, 42), (other, 73)] {
            state.set_segment(
                Segment::Cs,
                SegmentRegister::real_mode(Segment::Cs, entry.segment),
            );
            state.set_eip(u32::from(entry.offset));
            state.set_ax(0);
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(state.ax(), expected);
        }
    })
    .join()
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn block_requests_keep_their_own_code_segments() {
    pollster::block_on(compilation::run(requested_segments));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn block_requests_keep_their_own_code_segments() {
    compilation::run(requested_segments).await;
}

async fn jump_to_requested_block(service: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    memory.view().write(0x100, &[0xeb, 2]).unwrap(); // JMP 0104
    memory.view().write_u8(0x104, 0x40).unwrap(); // INC AX
    let cpu = machine.create_cpu(config, Some(service)).unwrap();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let state = runner.cpu().cpu_state();
        let mut compiler = runner.take_compiler().unwrap();
        let blocks = [(0x104, 1), (0x100, 2)].map(|(offset, byte_len)| BlockRequest {
            entry: FarPointer { segment: 0, offset },
            byte_len,
            instruction_limit: 1,
        });
        compiler
            .request(state.compiler_config(), blocks.to_vec())
            .unwrap();
        let code = compiler.recv().await.unwrap().unwrap();
        code.install(runner.cpu()).unwrap();

        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([0x104, 0x105].map(|key| {
                let exit = exit.clone();
                Hook::host(key, async move || exit.request())
            }))
            .await
            .unwrap();
        for (entry, stop, ax) in [(0x100, 0x105, 1), (0x104, 0x104, 0)] {
            state.set_eip(entry);
            state.set_ax(0);
            // Clear the worker wake so the jump can reach its local target without a checkpoint exit.
            let cpu = runner.cpu();
            cpu.interrupt_handle().take();
            assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!((state.eip(), state.ax()), (stop, ax));
        }
    })
    .join()
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn explicit_public_blocks_are_local_jump_targets() {
    pollster::block_on(compilation::run(jump_to_requested_block));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn explicit_public_blocks_are_local_jump_targets() {
    compilation::run(jump_to_requested_block).await;
}

async fn reject_host_page_table(service: &relocade_machine::CompilerService) {
    use relocade_machine::{ConfigurationError, Error, FunctionRequest};
    use relocade_x86::MemoryBackend;

    let (mut machine, config) = setup();
    let mut target = config.compiler_config();
    target.memory_backend = MemoryBackend::HostPageTable;
    let cpu = machine.create_cpu(config, Some(service)).unwrap();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let mut compiler = runner.take_compiler().unwrap();
        let block = request();
        let function = FunctionRequest {
            entry: block.entry,
            limits: Default::default(),
        };
        compiler.request(target, vec![block]).unwrap();
        compiler.request_functions(target, vec![function]).unwrap();
        for _ in 0..2 {
            assert!(matches!(
                compiler.recv().await.unwrap(),
                Err(Error::Configuration(
                    ConfigurationError::UnsupportedMemoryBackend
                ))
            ));
        }
    })
    .join()
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compiler_requests_require_the_physical_bus() {
    pollster::block_on(compilation::run(reject_host_page_table));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compiler_requests_require_the_physical_bus() {
    compilation::run(reject_host_page_table).await;
}
