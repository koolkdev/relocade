use super::*;
use crate::{Cpu, CpuConfig, DispatchEntry, MachineConfig};
use relocade_x86::{CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_abandoned_module() {
    let map =
        PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let memory = Memory::new(MachineConfig::new(0x1000, map).unwrap()).unwrap();
    memory.view().write(0x100, &[0x40]).unwrap();
    memory.view().write(0x110, &[0x43, 0xeb, 0xfd]).unwrap();
    let target = relocade_x86::Compiler::new(ExecutionProfile::Real16).target();
    let mut worker = CompilerWorker::start(&memory, target).await.unwrap();
    let (reply, ready) = oneshot::channel();
    worker
        .requests
        .try_send(Request {
            id: worker.next_id,
            blocks: vec![BlockRequest {
                address: 0x100,
                start_eip: 0x100,
                byte_len: 1,
                instruction_limit: 1,
            }],
            reply,
        })
        .unwrap();
    worker.next_id += 1;
    // The Rust reply follows posting the module. Abandon that posted resource
    // deterministically, then receive a different export from the same worker.
    ready.await.unwrap().unwrap();
    let compiled = worker
        .compile_blocks(vec![BlockRequest {
            address: 0x110,
            start_eip: 0x110,
            byte_len: 3,
            instruction_limit: 2,
        }])
        .await
        .unwrap();
    worker.shutdown().await.unwrap();
    let config = CpuConfig::new(
        target,
        CpuState {
            eip: 0x110,
            segments: Segments::real_mode(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut cpu = Cpu::new(memory, config, Default::default()).await.unwrap();
    cpu.install_compiled_module(
        &compiled.module,
        &[DispatchEntry {
            address: 0x110,
            export: &compiled.entries[0].export,
        }],
    )
    .await
    .unwrap();
    cpu.run(1).await;
    assert_eq!(cpu.cpu_state().bx(), 1);
    assert_eq!(cpu.cpu_state().eip(), 0x110);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn abandoned_posted_module_does_not_replace_next_result() {
    pollster::block_on(check_abandoned_module());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn abandoned_posted_module_does_not_replace_next_result() {
    check_abandoned_module().await;
}
