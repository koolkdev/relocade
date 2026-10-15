use super::*;
use wasm86_runtime::SliceStop;
use wasm86_x86::{Exception, ExecutionExit, Segment, StoredSegment};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn linear_aliases_share_functions_and_other_machines_do_not() {
    let mut machine = machine(&[0x40]).await;
    let mut other = super::machine(&[0x40]).await;
    let calls = count_calls(&mut machine, 0x100, HostAction::Continue).await;
    return_frame(&mut machine, 0x200);
    let mut cpu = machine.cpu();
    cpu.segments.cs = StoredSegment::real_mode(Segment::Cs, 0x10);
    cpu.eip = 0;
    machine.set_cpu(cpu).unwrap();
    machine.run_slice(2).unwrap();
    other.run_slice(1).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(machine.cpu().eip, 0x200);
    assert_eq!(other.cpu().registers.eax, 1);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn replacement_and_removal_take_effect_on_fresh_entries() {
    let mut machine = machine(&[0xeb, 0xfe]).await; // JMP to itself
    let old = count_calls(&mut machine, 0x100, HostAction::Continue).await;
    return_frame(&mut machine, 0x100);
    machine.run_slice(2).unwrap();
    let new = count_calls(&mut machine, 0x100, HostAction::Continue).await;
    return_frame(&mut machine, 0x100);
    machine.run_slice(2).unwrap();
    assert!(machine.remove_entry(0x100).unwrap());
    assert!(!machine.remove_entry(0x100).unwrap());
    machine.run_slice(2).unwrap();
    assert_eq!((old.get(), new.get()), (1, 1));
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn cs_limit_fault_precedes_a_matching_host_entry() {
    let mut machine = machine(&[]).await;
    let calls = count_calls(&mut machine, 0x10000, HostAction::Continue).await;
    let mut cpu = machine.cpu();
    cpu.eip = 0x10000;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine.run_slice(2).unwrap().stop,
        SliceStop::Exited(ExecutionExit::Exception(Exception::GeneralProtection {
            error_code: 0
        }))
    );
    assert_eq!(calls.get(), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn execution_keys_precede_a20_and_backing_translation() {
    let mut machine = machine(&[]).await;
    machine.write_memory(0, &[0x40]).unwrap();
    machine
        .with_memory(|memory| memory.set_a20_enabled(false))
        .unwrap();
    let low = count_calls(&mut machine, 0, HostAction::Continue).await;
    let high = count_calls(&mut machine, 0x100000, HostAction::Continue).await;
    return_frame(&mut machine, 0x200);
    let mut cpu = machine.cpu();
    cpu.segments.cs = StoredSegment::real_mode(Segment::Cs, 0xffff);
    cpu.eip = 0x10;
    machine.set_cpu(cpu).unwrap();
    machine.run_slice(2).unwrap();
    assert_eq!((low.get(), high.get(), machine.cpu().eip), (0, 1, 0x200));
}
