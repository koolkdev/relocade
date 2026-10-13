use super::*;
use wasm86_runtime::{ExecutableModule, SliceOutcome, SliceStop, StepOutcome};
use wasm86_x86::{Compiler, ExecutionProfile};

async fn install_block(machine: &mut TestMachine, address: u32, code: &[u8], instructions: u32) {
    machine.write_memory(address, code).unwrap();
    let compiled = Compiler::new(ExecutionProfile::Real16)
        .with_execution_budget()
        .compile_block(address, code, instructions)
        .unwrap();
    let entry = compiled.entry.clone();
    let mut module = ExecutableModule::new(compiled);
    module.bind_entry(address, entry);
    machine.install_module(module).await.unwrap();
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn ordinary_compiled_blocks_and_interpreter_fallback_share_dispatch() {
    let mut machine = machine(&[]).await;
    // Compiled INC AX; JMP 0200, then interpreted INC BX; JMP 0100.
    install_block(&mut machine, 0x100, &[0x40, 0xe9, 0xfc, 0], 2).await;
    machine
        .write_memory(0x200, &[0x43, 0xe9, 0xfc, 0xfe])
        .unwrap();
    assert_eq!(
        machine.run_slice(40).unwrap(),
        SliceOutcome {
            work: 40,
            completed: 40,
            stop: SliceStop::WorkLimit,
        }
    );
    let cpu = machine.cpu();
    assert_eq!(
        (cpu.registers.eax, cpu.registers.ebx, cpu.eip),
        (10, 10, 0x100)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn stepping_cuts_a_compiled_block_at_one_retirement_including_rep() {
    let mut machine = machine(&[]).await;
    install_block(&mut machine, 0x100, &[0xf3, 0xaa, 0x43, 0x41], 3).await;
    let mut cpu = machine.cpu();
    cpu.registers.eax = 0x5a;
    cpu.registers.ecx = 4096;
    cpu.registers.edi = 0x2000;
    cpu.instruction_count = u32::MAX;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    let cpu = machine.cpu();
    assert_eq!(
        (
            cpu.eip,
            cpu.registers.ecx,
            cpu.registers.edi,
            cpu.registers.ebx,
            cpu.instruction_count
        ),
        (0x102, 0, 0x3000, 0, 0)
    );
    let mut bytes = [0; 4096];
    machine.read_memory(0x2000, &mut bytes).unwrap();
    assert!(bytes.iter().all(|&byte| byte == 0x5a));
    assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
    assert_eq!((machine.cpu().eip, machine.cpu().registers.ebx), (0x103, 1));
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn address_replacement_and_removal_apply_to_ordinary_callable_exports() {
    let mut machine = machine(&[]).await;
    install_block(&mut machine, 0x100, &[0x40, 0xeb, 0xfd], 2).await;
    machine.run_slice(2).unwrap();
    // Revoke the old snapshot before changing its bytes.
    assert!(machine.remove_entry(0x100).unwrap());
    install_block(&mut machine, 0x100, &[0x43, 0xeb, 0xfd], 2).await;
    machine.run_slice(2).unwrap();
    assert!(machine.remove_entry(0x100).unwrap());
    assert!(!machine.remove_entry(0x100).unwrap());
    machine.run_slice(2).unwrap(); // same code through the interpreter
    assert_eq!(
        (machine.cpu().registers.eax, machine.cpu().registers.ebx),
        (1, 2)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn lookup_probes_collisions_and_observes_table_growth() {
    let mut machine = machine(&[]).await;
    // One position-independent JMP AX function, bound at many linear addresses.
    // The instruction starts at EIP zero under every segment alias.
    let compiled = Compiler::new(ExecutionProfile::Real16)
        .with_execution_budget()
        .compile_block(0, &[0xff, 0xe0], 1)
        .unwrap();
    let name = compiled.entry.clone();
    let mut module = ExecutableModule::new(compiled);
    for segment in 0..8192 {
        module.bind_entry(segment << 4, name.clone());
    }
    machine.install_module(module).await.unwrap();
    for segment in [0, 1, 31, 4096, 8191] {
        let mut cpu = machine.cpu();
        cpu.segments.cs = wasm86_x86::StoredSegment::real_mode(wasm86_x86::Segment::Cs, segment);
        cpu.eip = 0;
        cpu.registers.eax = 0x50;
        machine.set_cpu(cpu).unwrap();
        // Uphold snapshot bytes at the alias's instruction address.
        machine
            .write_memory(u32::from(segment) << 4, &[0xff, 0xe0])
            .unwrap();
        assert_eq!(machine.step().unwrap(), StepOutcome::Completed);
        assert_eq!(machine.cpu().eip, 0x50);
    }
}
