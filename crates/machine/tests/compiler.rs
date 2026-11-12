#[path = "support/compilation.rs"]
mod compilation;

use relocade_machine::{BlockRequest, CpuConfig, Hook, Machine, MachineConfig, MemoryError};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::{future::pending, pin::pin};

#[path = "compiler/delivery.rs"]
mod delivery;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn setup() -> (Machine, CpuConfig) {
    let config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
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
        (0x1000..=0x1fff, PhysicalMapping::MMIO),
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

async fn connections(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    // An unused CPU must not block later connections or compiler shutdown.
    let _unused = machine.create_cpu(config, Some(compiler)).unwrap();
    let first = machine.create_cpu(config, Some(compiler)).unwrap();
    let mut other_target = config.execution_target();
    other_target.instruction_budgets = !other_target.instruction_budgets;
    let second = machine
        .create_cpu(
            CpuConfig::new(other_target, *config.cpu()).unwrap(),
            Some(compiler),
        )
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
                    first.install(runner.cpu()),
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
                code.install(cpu).unwrap();
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

async fn shutdown(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let (mut other_machine, other_config) = setup();
    machine
        .memory()
        .view()
        .write(0x100, &[0xb8, 42, 0, 0xeb, 0])
        .unwrap();
    let cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    let other_cpu = other_machine
        .create_cpu(other_config, Some(compiler))
        .unwrap();
    other_machine
        .memory()
        .view()
        .write(0x100, &[0xb8, 73, 0, 0xeb, 0])
        .unwrap();
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
        compiler.request(target, vec![request()]).unwrap();
        other_submitted.send(()).unwrap();
        assert!(matches!(
            compiler.recv().await.unwrap(),
            Err(relocade_machine::Error::Compile(_))
        ));
        assert_eq!(
            compiler.recv().await.unwrap().unwrap().blocks()[0].byte_len,
            5
        );
        assert!(compiler.recv().await.is_none());
    });
    let closing = async move {
        ready.await.unwrap();
        other_ready.await.unwrap();
        let (first, second) =
            futures_util::future::join(machine.shutdown(), other_machine.shutdown()).await;
        first.unwrap();
        second.unwrap();
    };
    let (result, other_result, ()) =
        futures_util::future::join3(execution, other_execution, closing).await;
    assert_eq!(result.unwrap(), Some(()));
    assert_eq!(other_result.unwrap(), Some(()));
}

async fn immediate_shutdown(compiler: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let _cpu = machine.create_cpu(config, Some(compiler)).unwrap();
    // No await gives startup a host turn before shutdown begins.
    machine.shutdown().await.unwrap();
}

async fn independent_machines(compiler: &relocade_machine::CompilerService) {
    let (mut first, first_config) = setup();
    let (mut second, second_config) = setup();
    let _unused = first.create_cpu(first_config, Some(compiler)).unwrap();
    let cpu = second.create_cpu(second_config, Some(compiler)).unwrap();
    first
        .memory()
        .view()
        .write(0x100, &[0xb8, 42, 0, 0xeb, 0])
        .unwrap();
    second
        .memory()
        .view()
        .write(0x100, &[0xb8, 73, 0, 0xeb, 0])
        .unwrap();
    first.shutdown().await.unwrap();

    let value = cpu
        .run_until(pending::<()>(), async move |mut runner| {
            let target = runner.cpu().cpu_state().execution_target();
            let mut compiler = runner.take_compiler().unwrap();
            compiler.request(target, vec![request()]).unwrap();
            let code = compiler.recv().await.unwrap().unwrap();
            let exit = runner.cpu().exit_handle();
            runner
                .cpu()
                .install([Hook::host(0x105, async move || exit.request())])
                .await
                .unwrap();
            code.install(runner.cpu()).unwrap();
            assert_eq!(
                runner.run_cpu().await,
                ExecutionExit::Host(HostExit::Return)
            );
            runner.cpu().cpu_state().ax()
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value, 73);
    second.shutdown().await.unwrap();
}

async fn service_shutdown() {
    let compiler = relocade_machine::CompilerService::start_with_workers(
        std::num::NonZeroUsize::new(1).unwrap(),
    )
    .unwrap();
    let (mut machine, config) = setup();
    let _first = machine.create_cpu(config, Some(&compiler)).unwrap();
    let _second = machine.create_cpu(config, Some(&compiler)).unwrap();
    // Retained CPU handles must not keep a dropped machine's connections open.
    drop(machine);
    // Joining must allow startup and pool cleanup to finish before returning.
    compiler.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn another_machine_can_compile_after_a_machine_shuts_down() {
    pollster::block_on(compilation::run(independent_machines));
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn another_machine_can_compile_after_a_machine_shuts_down() {
    compilation::run(independent_machines).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn service_shutdown_joins_startup_after_machine_drop() {
    pollster::block_on(service_shutdown());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn service_shutdown_joins_startup_after_machine_drop() {
    service_shutdown().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_shutdown_joins_compiler_startup() {
    pollster::block_on(compilation::run(immediate_shutdown));
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_shutdown_joins_compiler_startup() {
    compilation::run(immediate_shutdown).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compiler_serves_independent_cpu_connections_over_shared_memory() {
    pollster::block_on(compilation::run(connections));
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compiler_serves_independent_cpu_connections_over_shared_memory() {
    compilation::run(connections).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machines_sharing_a_service_finish_accepted_compilation_during_shutdown() {
    pollster::block_on(compilation::run(shutdown));
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machines_sharing_a_service_finish_accepted_compilation_during_shutdown() {
    compilation::run(shutdown).await;
}
