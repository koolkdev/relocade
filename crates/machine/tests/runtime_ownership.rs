mod support;

use relocade_machine::{Cpu, Hook, Machine, RuntimeState};
use relocade_machine::{CpuConfig, MachineConfig};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::ops::ControlFlow;

use relocade_wasm_runtime::Backend;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn backend<T: 'static>(state: T) -> Backend<RuntimeState<T>> {
    Backend::with_state(RuntimeState::new(state)).unwrap()
}

fn cpu(eax: u32) -> CpuConfig {
    let mut cpu = CpuState {
        eip: 0x100,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    cpu.registers.eax = eax;
    CpuConfig::new(Compiler::new(ExecutionProfile::Real16).target(), cpu).unwrap()
}

fn configuration() -> MachineConfig {
    MachineConfig::new(
        0x2000,
        PhysicalMemoryMap::new([(0..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

async fn check_private_cpus() {
    let mut first_backend = backend(10_u32);
    let space = Machine::new(&mut first_backend, configuration()).unwrap();
    assert_eq!(space.cpu_count(), 0);
    let mut first = space.create_cpu(first_backend, cpu(0)).await.unwrap();
    assert_eq!(space.cpu_count(), 1);
    first
        .context()
        .memory()
        .write(0x100, &[0x40, 0xa3, 0, 2, 0xeb, 0xfa])
        .unwrap();
    let mut second = space.create_cpu(backend(20_u32), cpu(100)).await.unwrap();
    assert_eq!(space.cpu_count(), 2);
    assert_ne!(first.id(), second.id());
    assert_eq!(first.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(space.memory().view().read_u16(0x200).unwrap(), 1);
    assert_eq!(second.context().cpu().eax(), 100);
    assert_eq!(second.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(space.memory().view().read_u16(0x200).unwrap(), 101);
    assert_eq!(first.view().eax(), 1);
    assert_eq!(first.view().instruction_count(), 3);
    assert_eq!(second.context().cpu().instruction_count(), 3);
    first
        .install_hooks([Hook::new(0x300, |call| {
            *call.state_mut() += 1;
            call.defer(async |call| {
                call.memory().write_u16(0x202, 0xabcd).unwrap();
                ControlFlow::Break(ExecutionExit::HostReturn)
            })
        })])
        .await
        .unwrap();
    first.view().set_eip(0x300);
    assert_eq!(first.run(1).unwrap(), ExecutionExit::Deferred);
    assert_eq!(second.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(first.run_async(1).await.unwrap(), ExecutionExit::HostReturn);
    assert_eq!((*first.state(), *second.state()), (11, 20));
    assert_eq!(space.memory().view().read_u16(0x200).unwrap(), 102);
    assert_eq!(space.memory().view().read_u16(0x202).unwrap(), 0xabcd);
    let mut shared = space.memory();
    let reader = shared.view();
    drop(space);
    assert_eq!(second.machine().cpu_count(), 2);
    drop(first);
    assert_eq!(second.machine().cpu_count(), 1);
    assert_eq!(second.run(1).unwrap(), ExecutionExit::BudgetExhausted);
    drop(second);
    assert_eq!(reader.read_u16(0x200).unwrap(), 103);
}

async fn check_shared_access() {
    let mut owner = backend(());
    let space = Machine::new(&mut owner, configuration()).unwrap();
    // Address spaces can apply different mappings to the same allocation.
    let alias = Machine::with_backing(
        MachineConfig::new(
            0x2000,
            PhysicalMemoryMap::new([
                (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
                (0x1000..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 }),
            ])
            .unwrap(),
        )
        .unwrap(),
        space.memory().backing().clone(),
    )
    .unwrap();
    let mut peer = alias.create_cpu(backend(0_u32), cpu(100)).await.unwrap();
    peer.context()
        .memory()
        .write(0x100, &[0x40, 0xeb, 0xfd])
        .unwrap();
    let mut first = space.create_cpu(backend(peer), cpu(0)).await.unwrap();
    first
        .install_hooks([Hook::new(0x100, |call| {
            call.memory().write_u16(0x200, 0x1234).unwrap();
            let peer: &mut Cpu<Backend<RuntimeState<u32>>> = call.state_mut();
            assert_eq!(peer.memory().view().read_u16(0x1200).unwrap(), 0x1234);
            assert_eq!(peer.run(1).unwrap(), ExecutionExit::BudgetExhausted);
            assert_eq!(peer.context().cpu().eax(), 101);
            peer.context().memory().write_u16(0x1200, 0xabcd).unwrap();
            peer.context()
                .memory()
                .write_backing(0x202, &[0x55])
                .unwrap();
            assert_eq!(peer.memory().view().read_u16(0x1200).unwrap(), 0xabcd);
            ControlFlow::Break(ExecutionExit::HostReturn)
        })])
        .await
        .unwrap();
    assert_eq!(first.run(1).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(space.memory().view().read_u16(0x200).unwrap(), 0xabcd);
    assert_eq!(space.memory().view().read_u8(0x202).unwrap(), 0x55);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn machine_cpus_retain_ownership_and_keep_state_and_calls_private() {
    pollster::block_on(check_private_cpus());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn machine_cpus_retain_ownership_and_keep_state_and_calls_private() {
    check_private_cpus().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_backing_allows_access_from_another_cpu() {
    pollster::block_on(check_shared_access());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shared_backing_allows_access_from_another_cpu() {
    check_shared_access().await;
}
