use super::*;
use relocade_machine::{DiscoveryLimits, FarPointer, FunctionRequest, IoBus, Memory};
use relocade_x86::{Segment, SegmentRegister};

// Releasing the replaced hook reenters the I/O runtime and writes to this PC's RAM.
struct WriteOnHookDrop {
    memory: Memory,
    io: IoBus,
}

impl Drop for WriteOnHookDrop {
    fn drop(&mut self) {
        let mut view = self.memory.view().with_io(&self.io);
        let open_bus = view.read_u8(0x1000).unwrap();
        // The root at physical 0x100 is backed by 0x1100 in this machine.
        if !self.memory.installed_code_at(0x1100).is_empty() {
            view.write_u8(0x300, open_bus).unwrap();
        }
    }
}

async fn execute_branches(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let mut state = *config.cpu();
    state.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x10);
    state.eip = 0;
    state.registers.esp = 0x800;
    state.registers.ebx = 0x20;
    let config = CpuConfig::new(config.compiler_config(), state).unwrap();
    machine
        .memory()
        .view()
        .write(
            0x100,
            &[
                0xb9, 2, 0, // MOV CX,2
                0x49, 0x75, 0xfd, // DEC CX; JNZ 0003
                0xe8, 7, 0, // CALL 0010
                0xb8, 42, 0, // MOV AX,42
                0xff, 0xe3, // JMP BX, leaving the discovered region
            ],
        )
        .unwrap();
    machine.memory().view().write(0x110, &[0x42, 0xc3]).unwrap(); // INC DX; RET
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let result = ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let target = runner.cpu().cpu_state().compiler_config();
        let mut compiler = runner.take_compiler().unwrap();
        compiler
            .request_functions(
                target,
                [0, 3]
                    .map(|offset| FunctionRequest {
                        entry: FarPointer {
                            segment: 0x10,
                            offset,
                        },
                        limits: DiscoveryLimits::default(),
                    })
                    .into(),
            )
            .unwrap();
        let code = compiler.recv().await.unwrap().unwrap();
        assert_eq!(
            code.blocks()
                .iter()
                .map(|b| (b.address.eip(), b.byte_len))
                .collect::<Vec<_>>(),
            [(0, 3), (3, 3), (9, 5)]
        );
        let exit = runner.cpu().exit_handle();
        let root_exit = exit.clone();
        let memory = runner.cpu().memory();
        let cleanup = WriteOnHookDrop {
            memory: memory.clone(),
            io: runner.cpu().io(),
        };
        runner
            .cpu()
            .install([
                Hook::host(0x0010_0000, async move || {
                    let _keep_alive = &cleanup;
                    root_exit.request();
                }),
                Hook::host(0x0010_0020, async move || exit.request()),
            ])
            .await
            .unwrap();

        // The CALL at IP 0006 is internal to the module. Changing its displacement
        // through the A20 alias must reject the reply and preserve the root hook.
        memory.view().write_u8(0x10_0107, 8).unwrap();
        assert!(matches!(
            code.install(runner.cpu()),
            Err(relocade_machine::Error::StaleCompilation)
        ));
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(runner.cpu().cpu_state().instruction_count(), 0);
        assert_eq!(memory.view().read_u8(0x300).unwrap(), 0);

        memory.view().write_u8(0x107, 7).unwrap();
        // Discovery read beyond JMP BX, but that byte is not in the module.
        memory.view().write_u8(0x10e, 0x90).unwrap();
        code.install(runner.cpu()).unwrap();
        assert_eq!(memory.view().read_u8(0x300).unwrap(), 0xff);
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        let state = runner.cpu().cpu_state();
        assert_eq!(state.ax(), 42);
        assert_eq!(state.cx(), 0);
        assert_eq!(state.dx(), 1);
        assert_eq!(state.sp(), 0x800);
        assert_eq!(state.instruction_count(), 10);
    })
    .join()
    .await;
    machine.shutdown().await.unwrap();
    assert_eq!(result.unwrap(), Some(()));
}

async fn execute_batch(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let mut state = *config.cpu();
    state.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x10);
    state.eip = 0;
    state.registers.esp = 0x800;
    state.registers.ebx = 0x20;
    let config = CpuConfig::new(config.compiler_config(), state).unwrap();
    machine
        .memory()
        .view()
        .write(
            0x100,
            &[
                0xb8, 42, 0, // MOV AX,42
                0x9a, 0, 0, 0x20, 0, // CALL FAR 0020:0000
                0xff, 0xe3, // JMP BX to the host exit
            ],
        )
        .unwrap();
    machine.memory().view().write(0x200, &[0x42, 0xcb]).unwrap(); // INC DX; RETF
    machine.memory().view().write(0x300, &[0xf1]).unwrap(); // Unsupported root
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let result = ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let target = runner.cpu().cpu_state().compiler_config();
        let mut compiler = runner.take_compiler().unwrap();
        let request = |segment| FunctionRequest {
            entry: FarPointer { segment, offset: 0 },
            limits: DiscoveryLimits::default(),
        };
        // Both valid roots survive a rejected root between them.
        compiler
            .request_functions(target, vec![request(0x10), request(0x30), request(0x20)])
            .unwrap();
        let code = compiler.recv().await.unwrap().unwrap();
        assert_eq!(
            code.blocks()
                .iter()
                .map(|block| (block.address.eip(), block.byte_len))
                .collect::<Vec<_>>(),
            [(0, 8), (8, 2), (0, 2)]
        );
        assert_eq!(code.ranges(), &[0x100..0x10a, 0x200..0x202]);
        compiler
            .request_functions(target, vec![request(0x30)])
            .unwrap();
        assert!(matches!(
            compiler.recv().await.unwrap(),
            Err(relocade_machine::Error::Compile(_))
        ));
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([
                Hook::host(0x0010_0000, async || panic!("caller was not installed")),
                Hook::host(0x0020_0000, async || panic!("callee was not installed")),
                Hook::host(0x0010_0020, async move || exit.request()),
            ])
            .await
            .unwrap();
        code.install(runner.cpu()).unwrap();
        for address in [0x100, 0x109, 0x200, 0x201] {
            assert_eq!(runner.cpu().compiled_modules_at(address).count(), 1);
        }
        for address in [0xff, 0x10a, 0x180, 0x202, 0x300] {
            assert_eq!(runner.cpu().compiled_modules_at(address).count(), 0);
        }
        // Replacement leaves the old module owned: both versions depend on
        // these bytes, even though only the newest exports are dispatched.
        code.install(runner.cpu()).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 2);

        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        let state = runner.cpu().cpu_state();
        assert_eq!((state.ax(), state.dx(), state.sp()), (42, 1, 0x800));
        assert_eq!(state.instruction_count(), 5);
    })
    .join()
    .await;
    machine.shutdown().await.unwrap();
    assert_eq!(result.unwrap(), Some(()));
}

async fn execute_aliases(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    machine
        .memory()
        .view()
        .write(
            0x100,
            &[
                0x83, 0xfc, 1, // CMP SP,1 clears ZF with the fixture's stack
                0x0f, 0x84, 0xf9, 0, // JZ 0200 makes the callee reachable from this root
                0xe8, 0xf6, 0, // CALL 0200 must still honor callee replacement
                0xff, 0xe3, // JMP BX to the CS:IP exit hook
            ],
        )
        .unwrap();
    machine
        .memory()
        .view()
        .write(
            0x200,
            &[
                0xe8, 0, 0, // CALL next: observe the callee's CS-relative return IP
                0x58, 0xc3, // POP AX; RET to the caller
            ],
        )
        .unwrap();
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let target = runner.cpu().cpu_state().compiler_config();
        let mut compiler = runner.take_compiler().unwrap();
        compiler
            .request_functions(
                target,
                [(0, 0x100), (1, 0xf0), (0, 0x200), (1, 0x1f0)]
                    .map(|(segment, offset)| FunctionRequest {
                        entry: FarPointer { segment, offset },
                        limits: DiscoveryLimits::default(),
                    })
                    .into(),
            )
            .unwrap();
        let code = compiler.recv().await.unwrap().unwrap();
        let exit = runner.cpu().exit_handle();
        let alias_exit = exit.clone();
        runner
            .cpu()
            .install([
                // Neither return address was a requested root. Installing the
                // discovered continuations must replace these dispatch entries.
                Hook::host(0x10a, async || {
                    panic!("RET missed the compiled continuation")
                }),
                Hook::host(0x0001_00fa, async || {
                    panic!("RET missed the alias continuation")
                }),
                Hook::host(0x120, async move || exit.request()),
                Hook::host(0x0001_0110, async move || alias_exit.request()),
            ])
            .await
            .unwrap();
        code.install(runner.cpu()).unwrap();
        for phase in 0..3 {
            if phase == 1 {
                // Replacing one CS:IP must leave its alias unchanged.
                let exit = runner.cpu().exit_handle();
                runner
                    .cpu()
                    .install([Hook::host(0x200, async move || exit.request())])
                    .await
                    .unwrap();
            } else if phase == 2 {
                code.install(runner.cpu()).unwrap();
            }
            for (segment, offset, return_ip) in [(0, 0x100, 0x203), (1, 0xf0, 0x1f3)] {
                let state = runner.cpu().cpu_state();
                state.set_segment(
                    Segment::Cs,
                    SegmentRegister::real_mode(Segment::Cs, segment),
                );
                state.set_eip(offset);
                let initial_count = state.instruction_count();
                state.set_sp(0x800);
                state.set_ax(0);
                state.set_bx(0x120 - (segment << 4));
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(HostExit::Return)
                );
                let state = runner.cpu().cpu_state();
                let pushed_ip = runner.cpu().memory().view().read_u16(0x7fe).unwrap();
                assert_eq!(u32::from(pushed_ip), offset + 10);
                let retired = state.instruction_count() - initial_count;
                if phase == 1 && segment == 0 {
                    assert_eq!((state.ax(), state.sp(), retired), (0, 0x7fe, 3));
                } else {
                    assert_eq!((state.ax(), state.sp(), retired), (return_ip, 0x800, 7));
                }
            }
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
fn real_mode_aliases_keep_separate_entries_and_hooks() {
    pollster::block_on(compilation::run(execute_aliases));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn real_mode_aliases_keep_separate_entries_and_hooks() {
    compilation::run(execute_aliases).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn discovered_blocks_run_branches_and_an_interpreted_callee() {
    pollster::block_on(compilation::run(execute_branches));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn discovered_blocks_run_branches_and_an_interpreted_callee() {
    compilation::run(execute_branches).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn function_batch_shares_a_module_across_segments_and_skips_rejected_entries() {
    pollster::block_on(compilation::run(execute_batch));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn function_batch_shares_a_module_across_segments_and_skips_rejected_entries() {
    compilation::run(execute_batch).await;
}
