use relocade_machine::{BlockRequest, CpuConfig, Hook, Machine, MachineConfig, MemoryError};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{future::pending, pin::pin};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn setup() -> (Machine, CpuConfig) {
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
    )
    .unwrap();
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        ),
        (0x1000..=0x1fff, PhysicalMapping::Mmio),
    ])
    .unwrap();
    let machine = Machine::new(MachineConfig::new(0x2000, map).unwrap()).unwrap();
    (machine, config)
}

fn request() -> BlockRequest {
    BlockRequest {
        address: 0x100,
        start_eip: 0x100,
        byte_len: 5,
        instruction_limit: 2,
    }
}

async fn sessions() {
    let (mut machine, config) = setup();
    // An unused CPU must not block later connections or compiler shutdown.
    let _unused = machine.create_cpu(config).unwrap();
    let first = machine.create_cpu(config).unwrap();
    let mut other_target = config.execution_target();
    other_target.instruction_budgets = !other_target.instruction_budgets;
    let second = machine
        .create_cpu(CpuConfig::new(other_target, *config.cpu()).unwrap())
        .unwrap();
    for (cpu, value) in [(first, 42_u16), (second, 73)] {
        machine
            .memory()
            .view()
            .write(0x100, &[0xb8, value as u8, 0, 0xeb, 0])
            .unwrap();
        let observed = cpu
            .run_until(pending::<()>(), async move |mut runner| {
                let target = runner.cpu().cpu_state().execution_target();
                let mut compiler = runner.take_compiler().unwrap();
                assert!(runner.take_compiler().is_none());
                for address in [0x1000, 0x2000] {
                    compiler
                        .request(
                            target,
                            vec![BlockRequest {
                                address,
                                ..request()
                            }],
                        )
                        .unwrap();
                    let result = compiler.recv().await.unwrap();
                    assert!(matches!(
                        result,
                        Err(relocade_machine::Error::Memory(MemoryError::NotRamOrRom))
                    ));
                }
                compiler
                    .request(
                        target,
                        vec![BlockRequest {
                            address: u32::MAX,
                            ..request()
                        }],
                    )
                    .unwrap();
                let result = compiler.recv().await.unwrap();
                assert!(matches!(
                    result,
                    Err(relocade_machine::Error::Memory(MemoryError::AddressRange))
                ));
                compiler
                    .request(
                        target,
                        vec![BlockRequest {
                            instruction_limit: 0,
                            ..request()
                        }],
                    )
                    .unwrap();
                let invalid = compiler.recv().await.unwrap();
                assert!(matches!(invalid, Err(relocade_machine::Error::Compile(_))));
                let mut other_target = target;
                other_target.instruction_budgets = !target.instruction_budgets;
                compiler
                    .request(
                        other_target,
                        vec![BlockRequest {
                            instruction_limit: 1,
                            ..request()
                        }],
                    )
                    .unwrap();
                // Cancelling a receive leaves the first result in the stream.
                let early = {
                    let mut receive = pin!(compiler.recv());
                    futures_lite::future::poll_once(receive.as_mut()).await
                };
                compiler.request(target, vec![request()]).unwrap();
                let first = match early {
                    Some(result) => result,
                    None => compiler.recv().await,
                }
                .unwrap()
                .unwrap();
                assert_eq!(first.blocks()[0].byte_len, 3);
                assert!(matches!(
                    first.install(runner.cpu()).await,
                    Err(relocade_machine::Error::CompilationTarget)
                ));
                let code = compiler.recv().await.unwrap().unwrap();
                assert_eq!(code.blocks()[0].byte_len, 5);
                let cpu = runner.cpu();
                let exit = cpu.exit_handle();
                cpu.install([
                    Hook::host(0x100, async || {
                        panic!("compiled entry must replace this hook")
                    }),
                    Hook::host(0x105, async move || exit.request()),
                ])
                .await
                .unwrap();
                code.install(cpu).await.unwrap();
                assert_eq!(
                    runner.run_cpu().await,
                    ExecutionExit::Host(HostExit::Return)
                );
                runner.cpu().cpu_state().ax()
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed, value);
    }
    machine.shutdown().await.unwrap();
}

async fn shutdown() {
    let (mut machine, config) = setup();
    machine
        .memory()
        .view()
        .write(0x100, &[0xb8, 42, 0, 0xeb, 0])
        .unwrap();
    let cpu = machine.create_cpu(config).unwrap();
    let other_cpu = machine.create_cpu(config).unwrap();
    let (submitted, ready) = futures_channel::oneshot::channel();
    let (other_submitted, other_ready) = futures_channel::oneshot::channel();
    let execution = cpu.run_until(pending::<()>(), async move |mut runner| {
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        compiler
            .request(
                target,
                vec![BlockRequest {
                    instruction_limit: 1,
                    ..request()
                }],
            )
            .unwrap();
        compiler.request(target, vec![request()]).unwrap();
        submitted.send(()).unwrap();
        assert_eq!(
            compiler.recv().await.unwrap().unwrap().blocks()[0].byte_len,
            3
        );
        assert_eq!(
            compiler.recv().await.unwrap().unwrap().blocks()[0].byte_len,
            5
        );
        assert!(compiler.recv().await.is_none());
        assert!(compiler.request(target, vec![request()]).is_err());
    });
    let other_execution = other_cpu.run_until(pending::<()>(), async move |mut runner| {
        let target = runner.cpu().cpu_state().execution_target();
        let mut compiler = runner.take_compiler().unwrap();
        compiler
            .request(
                target,
                vec![BlockRequest {
                    instruction_limit: 0,
                    ..request()
                }],
            )
            .unwrap();
        other_submitted.send(()).unwrap();
        assert!(matches!(
            compiler.recv().await.unwrap(),
            Err(relocade_machine::Error::Compile(_))
        ));
        assert!(compiler.recv().await.is_none());
    });
    let closing = async move {
        ready.await.unwrap();
        other_ready.await.unwrap();
        machine.shutdown().await.unwrap();
    };
    let (result, other_result, ()) =
        futures_util::future::join3(execution, other_execution, closing).await;
    assert_eq!(result.unwrap(), Some(()));
    assert_eq!(other_result.unwrap(), Some(()));
}

async fn immediate_shutdown() {
    let (mut machine, config) = setup();
    let _cpu = machine.create_cpu(config).unwrap();
    // No await gives startup a host turn before shutdown begins.
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_shutdown_joins_compiler_startup() {
    pollster::block_on(immediate_shutdown());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_shutdown_joins_compiler_startup() {
    immediate_shutdown().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compiler_serves_independent_cpu_sessions_over_shared_memory() {
    pollster::block_on(sessions());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compiler_serves_independent_cpu_sessions_over_shared_memory() {
    sessions().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_shutdown_finishes_accepted_compilation() {
    pollster::block_on(shutdown());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_shutdown_finishes_accepted_compilation() {
    shutdown().await;
}
