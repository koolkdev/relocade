use relocade_machine::MachineConfig;
use relocade_runtime::{BlockRequest, HotEntry, Runtime, RuntimeState};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HotnessTable, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister, Segments,
};
use std::num::NonZeroU16;

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_hotness() {
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x1000,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let backend = Backend::with_state(RuntimeState::new(())).unwrap();
    #[cfg(target_arch = "wasm32")]
    let backend = Backend::with_state(RuntimeState::new(()));
    let mut runtime = Runtime::new(backend, config).await.unwrap();
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    assert_eq!(runtime.hot_threshold(), None);
    assert_eq!(runtime.run(4).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.take_hot_request(), None);
    runtime.set_hot_threshold(NonZeroU16::new(3));
    assert_eq!(runtime.run(100).unwrap(), ExecutionExit::CompileRequest);
    assert_eq!(runtime.machine().cpu().ax(), 4);
    assert_eq!(runtime.machine().cpu().instruction_count(), 8);
    assert_eq!(runtime.machine().cpu().eip(), 0x1000);
    let before = runtime.machine().cpu().snapshot();
    assert_eq!(runtime.run(0).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(
        runtime.take_hot_request(),
        Some(HotEntry {
            code_base: 0,
            start_eip: 0x1000
        })
    );
    assert_eq!(runtime.take_hot_request(), None);
    assert_eq!(runtime.machine().cpu().snapshot(), before);
    assert_eq!(runtime.run(6).unwrap(), ExecutionExit::BudgetExhausted);

    // Declining a request permits a full cooldown of progress, then retries.
    runtime.set_hot_threshold(NonZeroU16::new(1));
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::CompileRequest);
    runtime.take_hot_request().unwrap();
    let count = runtime.machine().cpu().instruction_count();
    assert_eq!(
        runtime
            .run(u64::from(HotnessTable::RETRY_VISITS) * 2)
            .unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(
        runtime.machine().cpu().instruction_count(),
        count + u64::from(HotnessTable::RETRY_VISITS) * 2
    );
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::CompileRequest);
    assert_eq!(
        runtime.machine().cpu().instruction_count(),
        count + u64::from(HotnessTable::RETRY_VISITS) * 2
    );
    runtime.take_hot_request().unwrap();

    // Equal linear addresses have the same hash bucket, but different CS/EIP
    // identities must not combine their counts.
    runtime.set_hot_threshold(NonZeroU16::new(3));
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    runtime
        .machine()
        .cpu()
        .set_segment(Segment::Cs, SegmentRegister::real_mode(Segment::Cs, 0x100));
    runtime.machine().cpu().set_eip(0);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::CompileRequest);
    assert_eq!(
        runtime.take_hot_request(),
        Some(HotEntry {
            code_base: 0x1000,
            start_eip: 0
        })
    );
    runtime
        .machine()
        .cpu()
        .set_segment(Segment::Cs, Segments::real_mode().cs);
    runtime.machine().cpu().set_eip(0x1000);

    runtime.set_hot_threshold(NonZeroU16::new(1));
    let requests = [BlockRequest {
        start_eip: 0x1000,
        instruction_limit: 16,
    }];
    let group = runtime.prepare_group(&requests).unwrap().unwrap();
    runtime.install_group(group).await.unwrap().unwrap();
    assert_eq!(runtime.run(20).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.take_hot_request(), None);

    // A watched read forces a compiled guard into the raw interpreter. Returning
    // to that block repeatedly must not increment the dispatch-miss counters.
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0xa1, 0, 0x10, 0xeb, 0xfb])
        .unwrap();
    let group = runtime.prepare_group(&requests).unwrap().unwrap();
    runtime.install_group(group).await.unwrap().unwrap();
    assert_eq!(runtime.run(20).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().ax(), 0xa1);
    assert_eq!(runtime.take_hot_request(), None);
    runtime.set_hot_threshold(None);
    assert_eq!(runtime.hot_threshold(), None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn profiling_requests_preserve_state_identity_and_progress() {
    pollster::block_on(check_hotness());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn profiling_requests_preserve_state_identity_and_progress() {
    check_hotness().await;
}
