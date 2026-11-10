use futures_channel::oneshot;
use futures_lite::future::{or, poll_once};
use relocade_machine::{
    run_session, yield_now, BlockRequest, Cpu, CpuConfig, DispatchEntry, MachineConfig, Memory,
    Runner,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{future::pending, pin::pin};

async fn cpu(runner: &Runner) -> Cpu {
    let map =
        PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let memory = Memory::new(MachineConfig::new(0x1000, map).unwrap()).unwrap();
    memory.view().write(0x100, &[0x40, 0xeb, 0xfd]).unwrap();
    let target = Compiler::new(ExecutionProfile::Real16)
        .with_instruction_budgets(false)
        .target();
    runner
        .create_cpu(
            memory,
            CpuConfig::new(
                target,
                CpuState {
                    eip: 0x100,
                    segments: Segments::real_mode(),
                    ..Default::default()
                },
            )
            .unwrap(),
            Default::default(),
        )
        .await
        .unwrap()
}

fn block(address: u32) -> Vec<BlockRequest> {
    vec![BlockRequest {
        address,
        start_eip: address,
        byte_len: 4,
        instruction_limit: 1,
    }]
}

async fn managed_compilation() {
    let result = run_session(pending::<()>(), async |runner| {
        assert!(runner.compile_blocks(block(0x200)).await.is_err());
        let mut cpu = cpu(runner).await;
        // Registration must retain the existing backing, including later writes.
        cpu.memory()
            .view()
            .write(0x200, &[0x43, 0x41, 0xeb, 0xfc])
            .unwrap(); // INC BX; INC CX; JMP 200.
        let state = cpu.cpu_state();
        let code = or(
            async {
                while state.instruction_count() == 0 {
                    yield_now().await;
                }
                runner.compile_blocks(block(0x200)).await.unwrap()
            },
            async {
                runner.run_cpu(&mut cpu).await;
                panic!("guest loop must run until compilation finishes");
            },
        )
        .await;
        assert!(state.instruction_count() > 0);
        assert_eq!(code.execution_target, state.execution_target());
        cpu.install_compiled_module(
            &code.module,
            &[DispatchEntry {
                address: 0x200,
                export: &code.entries[0].export,
            }],
        )
        .await
        .unwrap();
        state.set_eip(0x200);
        cpu.interrupt_handle().request();
        cpu.run().await;
        // An interpreted block would also execute INC CX and the jump.
        assert_eq!((state.bx(), state.cx(), state.eip()), (1, 0, 0x201));

        let second = runner.compile_blocks(block(0x100)).await.unwrap();
        assert_eq!(second.entries[0].start_eip, 0x100);
        42
    })
    .await
    .unwrap();
    assert_eq!(result, Some(42));

    let (close, closing) = oneshot::channel();
    let result = run_session(closing, async move |runner| {
        let _cpu = cpu(runner).await;
        let mut compiling = pin!(runner.compile_blocks(block(0x100)));
        assert!(poll_once(compiling.as_mut()).await.is_none());
        close.send(()).unwrap();
        pending::<()>().await;
    })
    .await
    .unwrap();
    assert_eq!(result, None);

    assert_eq!(
        run_session(pending::<()>(), async |_runner| 7)
            .await
            .unwrap(),
        Some(7)
    );
    let closed = run_session(async {}, async |_runner| {
        panic!("a closed session must not start its callback");
    })
    .await
    .unwrap();
    assert_eq!(closed, None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn runtime_owns_compilation_and_session_cleanup() {
    pollster::block_on(managed_compilation());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn runtime_owns_compilation_and_session_cleanup() {
    managed_compilation().await;
}
