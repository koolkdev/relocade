//! Real Wasm-to-Wasm handoffs use the interpreter's own stopping boundaries.

use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Event, TestModule},
    x87::{complete_x87, stack_image, write_register_bits},
};
use relocade_x86::{
    compile_block_from_bytes, BlockSnapshot, CodeAddress, Compiler, CpuState, DecodedBlock,
    ExecutionExit, MemoryBackend, SegmentDefaultSize, SegmentProfile, SnapshotInstruction,
};

pub(super) fn interpreter_run() -> &'static TestModule {
    TestModule::interpreter_run_with_profile(SegmentProfile::Flat32)
}

#[test]
fn a_linked_call_redecodes_the_destination_when_its_width_differs() {
    let call = [0xe8, 0x0b, 0, 0, 0]; // CALL 1010, pushing 1005
    let mov = [0xb8, 0x34, 0x12, 0x78, 0x56];
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let blocks = compiler
        .compile_blocks(&[
            BlockSnapshot {
                address: CodeAddress::CsRelative(0x1000),
                code_default_size: SegmentDefaultSize::Bits32,
                bytes: &call,
                instruction_limit: 1,
            },
            BlockSnapshot {
                address: CodeAddress::CsRelative(0x1010),
                code_default_size: SegmentDefaultSize::Bits16,
                bytes: &mov,
                instruction_limit: 1,
            },
        ])
        .unwrap();
    let module = TestModule::from_bytes(
        &blocks.bytes,
        &blocks.entries[0].export,
        Some(blocks.execution_target),
    )
    .with_interpreter(TestModule::interpreter());
    let mut image = Image::new(&call);
    image.data(0x3010, &mov);
    image.map(9, 0x8000, true);
    image.cpu.registers.esp = 0x9004;
    image.cpu.instruction_count = 0;
    let mut input = image.input();
    input.call_target = Some((0x1010, blocks.entries[1].export.clone()));

    let mut after = image.cpu;
    after.registers.esp = 0x9000;
    after.registers.eax = 0x5678_1234;
    after.eip = 0x1015;
    after.instruction_count = 2;
    assert_eq!(
        module.observe(&input, 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[(0x8000, &0x1005_u32.to_le_bytes())],
                exit: Exit::Other(ExecutionExit::StepComplete),
            }]
        )
    );
}

#[test]
fn protected_physical_bus_blocks_use_aliases_and_restart_mmio_in_the_interpreter() {
    let store = [0xa3, 4, 0x40, 0, 0xd0]; // MOV [D0004004],EAX
    let load = [0x8b, 0x0d, 4, 0x50, 0, 0xd0]; // MOV ECX,[D0005004] through the alias
    let device = [0x89, 0x0d, 4, 0x60, 0, 0xe0]; // MOV [E0006004],ECX through MMIO
    let code = [&store[..], &load, &device].concat();
    let mut image = Image::new(&code);
    image.cpu.instruction_count = 0;
    image.cpu.registers.eax = 0x7856_3412;
    image.map(0xd0004, 0x8000, true);
    image.map(0xd0005, 0x8000, true);
    let mut input = image.input();
    input.machine.clear(); // Only the physical bus maps code and data.
    input.mmio_pages.push((0xe0006, 0x9000));
    input.observe_mmio = true;

    let start = image.cpu.eip;
    let compiler =
        Compiler::new(SegmentProfile::Flat32).with_memory_backend(MemoryBackend::PhysicalBus);
    let raw = compiler.compile_block(start, &code, 3).unwrap();
    let decode = |eip, bytes: &[u8]| {
        SnapshotInstruction::decode(bytes, eip, SegmentDefaultSize::Bits32).unwrap()
    };
    let mut block =
        DecodedBlock::new(CodeAddress::CsRelative(start), decode(start, &store), true).unwrap();
    block.push(decode(start + 5, &load)).unwrap();
    block.push(decode(start + 11, &device)).unwrap();
    let decoded = compiler.compile_decoded_blocks(vec![block]).unwrap();

    let mut restart = image.cpu;
    restart.eip = 0x100b;
    restart.instruction_count = 2;
    restart.registers.ecx = 0x7856_3412;
    let mut complete = restart;
    complete.eip = 0x1011;
    complete.instruction_count = 3;
    let value = 0x7856_3412_u32.to_le_bytes();
    let before_mmio = expected(
        &image,
        &[Step {
            cpu: restart,
            ram: &[(0x8004, &value)],
            exit: Exit::Interpret,
        }],
    );
    let mut after_mmio = expected(
        &image,
        &[Step {
            cpu: complete,
            ram: &[(0x8004, &value), (0x9004, &value)],
            exit: Exit::Other(ExecutionExit::StepComplete),
        }],
    );
    after_mmio.events.insert(
        0,
        Event::MmioWrite {
            address: 0xe000_6004,
            value: value.to_vec(),
        },
    );

    for module in [
        TestModule::new(&raw),
        TestModule::from_bytes(
            &decoded.bytes,
            &decoded.entries[0].export,
            Some(decoded.execution_target),
        ),
    ] {
        assert_eq!(module.observe(&input, 1), before_mmio);
        let linked =
            module.with_interpreter(TestModule::interpreter_with_target(compiler.target()));
        assert_eq!(linked.observe(&input, 1), after_mmio);
    }
}

fn subnormal_loaded(cpu: CpuState) -> CpuState {
    let mut cpu = complete_x87(cpu, 6, 0x0105);
    cpu.x87.data_offset = 0x4000;
    cpu.x87.data_selector = 0x23;
    cpu.x87.status.denormal = 1;
    cpu.x87.status.c1 = 0;
    cpu.x87.status.top = 7;
    cpu.x87.tag_word = 0x3fff;
    write_register_bits(&mut cpu, 7, (0x8000_0000_0000_0000, 0x3f6a));
    cpu
}

#[test]
fn handoff_publishes_prefix_and_interpreter_step_restarts_once() {
    let code = [
        0x8d, 0x40, 1, // LEA EAX,[EAX+1]
        0xa3, 0, 0x40, 0, 0, // MOV [0x4000],EAX
        0xd9, 0x05, 0, 0x40, 0, 0, // FLD dword [0x4000]
        0x8d, 0x40, 1, // The captured suffix is abandoned on handoff.
    ];
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.registers.eax = 0;
    image.map(4, 0x8000, true);
    let mut restart = image.cpu;
    restart.eip = 0x1008;
    restart.instruction_count = 1; // Two completed instructions wrap u64::MAX.
    restart.registers.eax = 1;
    for linked in [false, true] {
        let mut module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 4).unwrap());
        let (cpu, exit) = if linked {
            module = module.with_interpreter(TestModule::interpreter());
            let cpu = subnormal_loaded(restart);
            (cpu, Exit::Other(relocade_x86::ExecutionExit::StepComplete))
        } else {
            (restart, Exit::Interpret)
        };
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[(0x8000, &1_u32.to_le_bytes())],
                exit,
            }],
        );
        let mut input = image.input();
        // Unlinked handoff preserves the callback outcome; linked STEP completes directly.
        input.dispatch_return =
            relocade_x86::ExecutionExit::Exception(relocade_x86::Exception::PageFault {
                linear_address: 0x1234,
                error_code: 0,
            });
        let Event::Return { outcome, .. } = wanted.events.last_mut().unwrap() else {
            unreachable!()
        };
        if !linked {
            *outcome = crate::support::step::returned(input.dispatch_return);
        }
        assert_eq!(module.observe(&input, 1), wanted, "linked {linked}");
    }
}

#[test]
fn interpreter_run_passes_snapshot_limit_and_stops_at_branch_or_fault() {
    let code = [
        0xd9, 0x05, 0, 0x40, 0, 0, // Only FLD belongs to the compiled snapshot.
        0xd9, 0x1d, 0, 0x50, 0, 0, // FSTP dword [0x5000]
        0xeb, 0,    // JMP ends interpreter run.
        0xf1, // Unsupported successor must not be fetched.
    ];
    let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap())
        .with_interpreter(interpreter_run());
    for destination_present in [false, true] {
        let mut image = stack_image(&code, 0, 0xffff);
        image.map(4, 0x8000, false);
        image.data(0x8000, &1_u32.to_le_bytes());
        let mut cpu = subnormal_loaded(image.cpu);
        let exit = if destination_present {
            image.map(5, 0x9000, true);
            cpu = complete_x87(cpu, 6, 0x011d);
            cpu.x87.data_offset = 0x5000;
            cpu.x87.status.top = 0;
            cpu.x87.tag_word = 0xffff;
            cpu.eip = 0x100e;
            cpu.instruction_count = 2;
            Exit::Dispatch(cpu.eip)
        } else {
            Exit::PageFault {
                address: 0x5000,
                error: 2,
            }
        };
        let bits = 1_u32.to_le_bytes();
        let written = [(0x9000, &bits[..])];
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: if destination_present { &written } else { &[] },
                    exit,
                }]
            ),
            "destination present {destination_present}"
        );
    }
}

#[test]
fn timestamp_includes_retirement_across_interpreter_handoff() {
    let code = [
        0x90, // The JIT retires this prefix before its FLD specialization fails.
        0xd9, 0x05, 0, 0x40, 0, 0, 0x0f, 0x31, 0xeb, 0,
    ];
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.instruction_count = 0xffff_ffff;
    image.map(4, 0x8000, false);
    image.data(0x8000, &1_u32.to_le_bytes());
    let mut restart = image.cpu;
    restart.eip = 0x1001;
    restart.instruction_count = 0x1_0000_0000;
    let mut cpu = subnormal_loaded(restart);
    cpu.registers.eax = 1;
    cpu.registers.edx = 1;
    cpu.eip = 0x100b;
    cpu.instruction_count = 0x1_0000_0003;
    let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 4).unwrap())
        .with_interpreter(interpreter_run());
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(0x100b)
            }]
        )
    );
}
