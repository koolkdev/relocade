use super::*;
use relocade_machine::{CompilationBinding, DiscoveryLimits, FunctionRequest};
use relocade_x86::{Segment, SegmentRegister};

async fn patch_running_code(compiler: &relocade_machine::CompilerService) {
    let map =
        PhysicalMemoryMap::new([(0..=0x3fff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let mut machine = Machine::new(MachineConfig::new(0x4000, map).unwrap()).unwrap();
    let config = CpuConfig::pc(CpuState::real_mode()).unwrap();
    let memory = machine.memory();
    let mut view = memory.view();
    // CALL 0FFB; JMP 4000. The stack is on its own page so the compiled CALL uses its link.
    view.write(0x100, &[0xe8, 0xf8, 0x0e, 0xe9, 0xfa, 0x3e])
        .unwrap();
    // MOV AX,7; ADD AX,1; RET. The ADD immediate spans two physical pages.
    view.write(0xffb, &[0xb8, 7, 0, 0x05, 1, 0, 0xc3]).unwrap();
    // LOCK INC word [0FFF]; JMP BX. BX points at the compiled caller.
    view.write(0x200, &[0xf0, 0xff, 6, 0xff, 0x0f, 0xff, 0xe3])
        .unwrap();
    // REP MOVSB patches the immediately following MOV AX,7; JMP 4000.
    view.write(0x300, &[0xf3, 0xa4, 0xb8, 7, 0, 0xe9, 0xf8, 0x3c])
        .unwrap();
    view.write(0x500, &[0x34, 0x12]).unwrap();

    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let mut compiler = runner.take_compiler().unwrap();
        let state = runner.cpu().cpu_state();
        for offset in [0x100, 0xffb, 0x200, 0x300] {
            compiler
                .request_functions(
                    state.compiler_config(),
                    vec![FunctionRequest {
                        entry: FarPointer { segment: 0, offset },
                        limits: DiscoveryLimits::default(),
                    }],
                )
                .unwrap();
            let code = compiler.recv().await.unwrap().unwrap();
            assert!(code
                .blocks()
                .iter()
                .any(|b| b.address.eip() == u32::from(offset)));
            code.install(runner.cpu()).unwrap();
        }
        let exit = runner.cpu().exit_handle();
        runner
            .cpu()
            .install([Hook::host(0x4000, async move || exit.request())])
            .await
            .unwrap();

        state.set_sp(0x3800);
        state.set_bx(0x100);
        state.set_si(0x500);
        state.set_di(0x303);
        state.set_cx(2);
        for (ip, ax, instructions) in [(0x100, 8, 5), (0x200, 9, 7), (0x300, 0x1234, 3)] {
            let before = state.instruction_count();
            state.set_eip(ip);
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(state.ax(), ax);
            assert_eq!(state.instruction_count() - before, instructions);
            assert_eq!(state.sp(), 0x3800);
        }

        assert_eq!(runner.memory().view().read_u16(0xfff).unwrap(), 2);
        assert_eq!((state.si(), state.di(), state.cx()), (0x502, 0x305, 0));
        assert_eq!(runner.cpu().compiled_modules_at(0xffb).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x300).count(), 0);
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 1);
        assert_eq!(runner.cpu().compiled_modules_at(0x200).count(), 1);
    })
    .join()
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_writes_retire_linked_and_executing_code() {
    pollster::block_on(compilation::run(patch_running_code));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_writes_retire_linked_and_executing_code() {
    compilation::run(patch_running_code).await;
}

async fn patch_jump_target(service: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let memory = machine.memory();
    let mut view = memory.view();
    view.write(0x100, &[0xeb, 6]).unwrap(); // JMP 0108
    view.write(0x104, &[0xb0, 42, 0xff, 0xe3]).unwrap(); // MOV AL,42; JMP BX
    view.write(0x108, &[0x40, 0x05, 0x40, 0, 0xff, 0xe3])
        .unwrap(); // INC AX; ADD AX,0040; JMP BX

    let cpu = machine.create_cpu(config, Some(service)).unwrap();
    ExecutionWorker::spawn(cpu, SessionConfig::host(), async move |mut runner| {
        let memory = runner.memory();
        let mut compiler = runner.take_compiler().unwrap();
        let state = runner.cpu().cpu_state();
        let compiler_config = state.compiler_config();
        let alias = FarPointer {
            segment: 0x10,
            offset: 8,
        };
        let roots = [0x100, 0x104, alias.packed()].map(|key| FunctionRequest {
            entry: FarPointer::from_packed(key),
            limits: DiscoveryLimits::default(),
        });
        let mut functions = async || {
            compiler
                .request_functions(compiler_config, roots.to_vec())
                .unwrap();
            compiler.recv().await.unwrap().unwrap()
        };
        let original = functions().await;
        assert_eq!(original.ranges(), [0x100..0x102, 0x104..0x10e]);
        original.install(runner.cpu()).unwrap();

        // The first change through A20 retires the original module. The new ADD can compile.
        memory
            .view()
            .write(0x10_0108, &[0x40, 0x05, 0x40, 1, 0xff, 0xe3])
            .unwrap();
        let replacement = functions().await;
        assert_eq!(replacement.ranges(), original.ranges());
        assert!(replacement.bindings().any(|binding| matches!(
            binding, CompilationBinding::Export { entry, .. } if entry == alias
        )));
        replacement.install(runner.cpu()).unwrap();
        assert_eq!(runner.cpu().compiled_modules_at(0x108).count(), 1);

        let exit = runner.cpu().exit_handle();
        // Either code segment reaches its stop at offset 1000 by executing JMP BX.
        runner
            .cpu()
            .install([0x1000, 0x0010_1000].map(|key| {
                let exit = exit.clone();
                Hook::host(key, async move || exit.request())
            }))
            .await
            .unwrap();
        state.set_bx(0x1000);
        state.set_ax(0);
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(state.ax(), 0x141);

        // Change the same byte in the installed replacement, then restore the late reply's bytes.
        let late = functions().await;
        memory.view().write_u8(0x10b, 2).unwrap();
        memory.view().write_u8(0x10b, 1).unwrap();
        assert!(matches!(
            late.install(runner.cpu()),
            Err(relocade_machine::Error::StaleCompilation)
        ));

        let stable = functions().await;
        assert_eq!(stable.ranges(), [0x100..0x102, 0x104..0x108]);
        let mut exports = Vec::new();
        let mut interpreted = Vec::new();
        for binding in stable.bindings() {
            match binding {
                CompilationBinding::Export { entry, .. } => exports.push(entry.packed()),
                CompilationBinding::Interpreter(entry) => interpreted.push(entry),
            }
        }
        exports.sort_unstable();
        assert_eq!(exports, [0x100, 0x104]);
        // The private JMP target at 0000:0108 covers the same bytes as the requested alias.
        assert_eq!(interpreted, [alias]);

        runner
            .cpu()
            .install([Hook::host(alias.packed(), async move || exit.request())])
            .await
            .unwrap();
        stable.install(runner.cpu()).unwrap();

        // All three aliases read the patched byte. Only the ADD uses it.
        // At 010A the unchanged low immediate itself decodes as a one-byte INC AX.
        let block = |offset, byte_len| BlockRequest {
            entry: FarPointer {
                segment: 0x10,
                offset,
            },
            byte_len,
            instruction_limit: 1,
        };
        compiler
            .request(
                compiler_config,
                vec![block(8, 6), block(9, 5), block(10, 4)],
            )
            .unwrap();
        let split = compiler.recv().await.unwrap().unwrap();
        assert_eq!(split.ranges(), [0x108..0x109, 0x10a..0x10b]);

        // The compiled JMP must dispatch to the interpreter for the current target bytes.
        memory.view().write_u8(0x10b, 3).unwrap();
        state.set_eip(0x100);
        state.set_ax(0);
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(state.ax(), 0x341);
        assert_eq!(runner.cpu().compiled_modules_at(0x100).count(), 1);
        assert_eq!(runner.cpu().compiled_modules_at(0x108).count(), 0);

        // The delayed interpreter binding must preserve the newer hook at the public alias.
        state.set_segment(
            Segment::Cs,
            SegmentRegister::real_mode(Segment::Cs, alias.segment),
        );
        state.set_eip(u32::from(alias.offset));
        assert_eq!(
            runner.run_cpu().await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(state.eip(), u32::from(alias.offset));
    })
    .join()
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_patch_excludes_blocks_covering_the_changed_byte() {
    pollster::block_on(compilation::run(patch_jump_target));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn repeated_patch_excludes_blocks_covering_the_changed_byte() {
    compilation::run(patch_jump_target).await;
}
