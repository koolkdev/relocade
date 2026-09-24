//! Reenter the same compiled repeat from actual fault state after repairing its page.

use super::record;
use crate::support::{
    guest::{
        Exit, Machine,
        Permissions::{ReadOnly, ReadWrite},
    },
    step::{Argument, Engine, Event, Input, Outcome, TestModule},
};
use wasm86_x86::compile_block_from_bytes;

fn restart_after_source_page_repair(engine: Engine, opcode: u8) {
    let code = [0xf3, opcode];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for module in [TestModule::interpreter(), &block] {
        let mut machine = Machine::new(&code);
        machine.cpu.flags = record(0xfe);
        machine.cpu.instruction_count = 17;
        machine.cpu.registers.eax = 0xaabb_ccdd;
        machine.cpu.registers.ecx = 4;
        machine.cpu.registers.esi = 0x4ffe;
        machine.cpu.registers.edi = 0x7000;
        machine.memory(0x4ffe, &[0x12, 0x34], ReadOnly);
        machine.memory(0x7000, &[0xa5; 4], ReadWrite);
        let initial = machine.state();
        let fault = machine.run(module, engine);
        let mut expected = initial;
        expected.cpu.registers.ecx = 2;
        expected.cpu.registers.esi = 0x5000;
        if opcode == 0xa4 {
            expected.cpu.registers.edi = 0x7002;
            expected.memory.write(0x7000, &[0x12, 0x34]);
        } else {
            expected.cpu.registers.eax = 0xaabb_cc34;
        }
        assert_eq!(fault.state, expected);
        assert_eq!(
            fault.exit,
            Exit::PageFault {
                address: 0x5000,
                error: 0
            }
        );
        assert!(fault.dispatches.is_empty() && fault.machine_unchanged);

        // Restore the actual fault state with the missing page repaired and the
        // consumed source page unmapped, so replaying completed loads would fault.
        let mut machine = Machine::new(&code);
        machine.cpu = fault.state.cpu;
        machine.memory(0x5000, &[0x56, 0x78], ReadOnly);
        machine.memory(0x7000, &fault.state.memory.read(0x7000, 4), ReadWrite);
        let mut completed = machine.state();
        completed.cpu.registers.ecx = 0;
        completed.cpu.registers.esi = 0x5002;
        completed.cpu.eip = 0x1002;
        completed.cpu.instruction_count = 18;
        if opcode == 0xa4 {
            completed.cpu.registers.edi = 0x7004;
            completed.memory.write(0x7000, &[0x12, 0x34, 0x56, 0x78]);
        } else {
            completed.cpu.registers.eax = 0xaabb_cc78;
        }
        let actual = machine.run(module, engine);
        assert_eq!(actual.state, completed);
        assert_eq!(actual.exit, Exit::Dispatch(0x1002));
        assert_eq!(actual.dispatches, [(0x1002, completed)]);
        assert!(actual.machine_unchanged);
    }
}

fn restart_comparisons_after_page_repair(engine: Engine) {
    let observer = TestModule::new(&crate::state::compile_flag_observer().unwrap());
    for (opcode, prefix) in [(0xa6, 0xf3), (0xae, 0xf2)] {
        let code = [prefix, opcode];
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
        for module in [TestModule::interpreter(), &block] {
            let mut machine = Machine::new(&code);
            machine.cpu.flags = record(0xfe);
            machine.cpu.flags.status_source.kind = 9;
            machine.cpu.instruction_count = 17;
            machine.cpu.registers.eax = 5;
            machine.cpu.registers.ecx = 4;
            machine.cpu.registers.esi = 0x4000;
            machine.cpu.registers.edi = 0x7ffe;
            machine.memory(0x4000, &[5; 4], ReadOnly);
            machine.memory(0x7ffe, &[if prefix == 0xf3 { 5 } else { 3 }; 2], ReadOnly);
            let initial = machine.state();
            let fault = machine.run(module, engine);
            let mut expected = initial;
            expected.cpu.registers.ecx = 2;
            expected.cpu.registers.edi = 0x8000;
            if opcode == 0xa6 {
                expected.cpu.registers.esi = 0x4002;
            }
            assert_eq!(fault.state, expected);
            assert_eq!(
                fault.exit,
                Exit::PageFault {
                    address: 0x8000,
                    error: 0
                }
            );
            assert!(fault.dispatches.is_empty() && fault.machine_unchanged);

            machine.cpu = fault.state.cpu;
            // A restart from the original indices would now stop immediately.
            machine.memory(0x7ffe, &[if prefix == 0xf3 { 3 } else { 5 }; 2], ReadOnly);
            machine.memory(
                0x8000,
                if prefix == 0xf3 { &[5, 3] } else { &[3, 5] },
                ReadOnly,
            );
            let before_resume = machine.state();
            let mut expected = before_resume.clone();
            expected.cpu.registers.ecx = 0;
            expected.cpu.registers.edi = 0x8002;
            if opcode == 0xa6 {
                expected.cpu.registers.esi = 0x4004;
            }
            expected.cpu.eip = 0x1002;
            expected.cpu.instruction_count = 18;
            let actual = machine.run(module, engine);
            let cpu_bytes = actual.state.cpu.to_bytes();
            let observed = engine.observe(&observer, &Input::new(&cpu_bytes), 1);
            let Event::Return {
                outcome: Outcome::Returned(values),
                snapshot,
            } = &observed.events[0]
            else {
                panic!("flag observer did not return");
            };
            let bits = if prefix == 0xf3 {
                [0, 0, 0, 0, 0, 0]
            } else {
                [0, 1, 0, 1, 0, 0]
            };
            assert_eq!(values, &bits.map(Argument::I32));
            assert_eq!(snapshot.cpu, cpu_bytes);
            let before = before_resume.cpu.flags.bytes;
            let after = actual.state.cpu.flags.bytes;
            assert_eq!(
                [
                    after.tf,
                    after.df,
                    after.nt,
                    after.ac,
                    after.id,
                    after.reserved
                ],
                [
                    before.tf,
                    before.df,
                    before.nt,
                    before.ac,
                    before.id,
                    before.reserved
                ]
            );
            // The logical result is checked above; compare every other CPU and memory field.
            expected.cpu.flags = actual.state.cpu.flags;
            assert_eq!(actual.state, expected);
            assert_eq!(actual.exit, Exit::Dispatch(0x1002));
            assert_eq!(actual.dispatches, [(0x1002, expected)]);
            assert!(actual.machine_unchanged);
        }
    }
}

#[test]
fn repaired_transfer_resumes_after_completed_elements() {
    for opcode in [0xa4, 0xac] {
        restart_after_source_page_repair(Engine::Wasmtime, opcode);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn repaired_transfer_resumes_after_completed_elements_v8() {
    for opcode in [0xa4, 0xac] {
        restart_after_source_page_repair(Engine::V8, opcode);
    }
}

#[test]
fn repaired_comparison_resumes_with_remaining_count() {
    restart_comparisons_after_page_repair(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn repaired_comparison_resumes_with_remaining_count_v8() {
    restart_comparisons_after_page_repair(Engine::V8);
}
