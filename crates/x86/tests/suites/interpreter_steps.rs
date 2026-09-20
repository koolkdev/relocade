use crate::support::cases::{
    test_cases, InstructionCase as Case, Permissions::ReadOnly, RegisterExpectation::Exact,
};
use crate::support::machine::{check, Exit, Image, Step};
use crate::support::sequences::{test_sequences, Checkpoint, SequenceCase};
use crate::support::step;
use step::TestModule;
use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, CpuState, Gpr32};
use wasmparser::{ExternalKind, Parser, Payload, TypeRef, ValType, Validator};

const MOVES: [(&[u8], Gpr32, u32); 8] = [
    (&[0xb8, 0x78, 0x56, 0x34, 0x12], Gpr32::Eax, 0x1234_5678),
    (&[0xb9, 0, 0, 0, 0x80], Gpr32::Ecx, 0x8000_0000),
    (&[0xba, 0xff, 0xff, 0xff, 0xff], Gpr32::Edx, 0xffff_ffff),
    (&[0xbb, 0, 0, 0, 0], Gpr32::Ebx, 0),
    (&[0xbc, 0xf3, 0x0f, 0xb8, 0x66], Gpr32::Esp, 0x66b8_0ff3),
    (&[0xbd, 0xff, 0xff, 0xff, 0x7f], Gpr32::Ebp, 0x7fff_ffff),
    (&[0xbe, 0xef, 0xbe, 0xad, 0xde], Gpr32::Esi, 0xdead_beef),
    (&[0xbf, 0x21, 0x43, 0x65, 0x87], Gpr32::Edi, 0x8765_4321),
];

const REGISTERS: [(Gpr32, u32); 8] = [
    (Gpr32::Eax, 0x1111_1111),
    (Gpr32::Ecx, 0x2222_2222),
    (Gpr32::Edx, 0x3333_3333),
    (Gpr32::Ebx, 0x4444_4444),
    (Gpr32::Esp, 0x5555_5555),
    (Gpr32::Ebp, 0x6666_6666),
    (Gpr32::Esi, 0x7777_7777),
    (Gpr32::Edi, 0x8888_8888),
];

fn state(eip: u32) -> CpuState {
    let mut cpu = CpuState::filled(0xa5);
    cpu.segments = wasm86_x86::Segments::flat32();
    for (register, value) in REGISTERS {
        cpu.registers[register] = value;
    }
    cpu.eip = eip;
    cpu.instruction_count = 0xffff_ffff;
    cpu.reserved_tail = [0; 4];
    cpu
}

#[test]
fn interpreter_step_exposes_memory_dispatch_and_descriptor_query_abis() {
    let module = compile_interpreter_step(crate::SegmentProfile::Flat32).unwrap();
    assert_eq!(module.entry, "step");
    Validator::new().validate_all(&module.bytes).unwrap();
    let mut types = Vec::new();
    let mut memories = Vec::new();
    let mut functions = Vec::new();
    let mut imports = Vec::new();
    let mut exported = None;
    for payload in Parser::new(0).parse_all(&module.bytes) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                for ty in section.into_iter_err_on_gc_types() {
                    let ty = ty.unwrap();
                    types.push((ty.params().to_vec(), ty.results().to_vec()));
                }
            }
            Payload::ImportSection(section) => {
                for import in section {
                    let import = import.unwrap();
                    assert_eq!(import.module, "wasm86");
                    match import.ty {
                        TypeRef::Memory(memory) => {
                            assert!(!memory.memory64 && !memory.shared);
                            memories.push((import.name.to_owned(), memory.initial));
                        }
                        TypeRef::Func(signature) => {
                            imports.push((import.name.to_owned(), signature))
                        }
                        _ => panic!("unexpected resource import"),
                    }
                }
            }
            Payload::FunctionSection(section) => {
                functions.extend(section.into_iter().map(Result::unwrap))
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    assert_eq!(export.kind, ExternalKind::Func);
                    assert_eq!(export.name, "step");
                    assert!(exported.replace(export.index).is_none());
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        memories,
        [
            ("cpuState".into(), 1),
            ("guest".into(), 1),
            ("machine".into(), 64)
        ]
    );
    assert_eq!(imports.len(), 3);
    assert_eq!(imports[0].0, "dispatch");
    assert_eq!(
        types[imports[0].1 as usize],
        (vec![ValType::I32], vec![ValType::I64])
    );
    assert_eq!(imports[1].0, "resolveSegment");
    assert_eq!(
        types[imports[1].1 as usize],
        (vec![ValType::I32; 2], vec![ValType::I32; 6])
    );
    assert_eq!(imports[2].0, "segmentPermissions");
    assert_eq!(
        types[imports[2].1 as usize],
        (vec![ValType::I32], vec![ValType::I32])
    );
    let entry = exported.unwrap() as usize - imports.len();
    assert_eq!(
        types[functions[entry] as usize],
        (vec![], vec![ValType::I64])
    );
}

fn register_moves() -> Vec<Case> {
    let mut cases = Vec::new();
    for (source, &(_, value)) in REGISTERS.iter().enumerate() {
        for (destination, &(register, _)) in REGISTERS.iter().enumerate() {
            for code in [
                [0x89, 0xc0 | ((source as u8) << 3) | destination as u8],
                [0x8b, 0xc0 | ((destination as u8) << 3) | source as u8],
            ] {
                cases.push(
                    Case::preserving_flags(
                        format!(
                            "MOV opcode {:02x}, source {source}, destination {destination}",
                            code[0]
                        ),
                        &code,
                    )
                    .at(0x1234)
                    .initial_registers(&REGISTERS)
                    .expect_register(register, Exact(value)),
                );
            }
        }
    }
    for &(code, register, value) in &MOVES {
        cases.push(
            Case::preserving_flags(format!("MOV {register:?}, immediate {value:#x}"), code)
                .at(0x1234)
                .initial_registers(&REGISTERS)
                .expect_register(register, Exact(value)),
        );
    }
    cases
}

#[rustfmt::skip]
fn forwarded_values() -> Vec<SequenceCase> {
    vec![
        SequenceCase::preserving_flags("register rotation retains source values").initial_registers(&REGISTERS)
            .step(Checkpoint::preserving_flags(&[0x89, 0xc2]).register(Gpr32::Edx, 0x1111_1111))
            .step(Checkpoint::preserving_flags(&[0x8b, 0xc1]).register(Gpr32::Eax, 0x2222_2222))
            .step(Checkpoint::preserving_flags(&[0x89, 0xd1]).register(Gpr32::Ecx, 0x1111_1111))
            .step(Checkpoint::preserving_flags(&[0x8b, 0xf8]).register(Gpr32::Edi, 0x2222_2222))
            .step(Checkpoint::preserving_flags(&[0x89, 0xce]).register(Gpr32::Esi, 0x1111_1111))
            .step(Checkpoint::preserving_flags(&[0x8b, 0xda]).register(Gpr32::Ebx, 0x1111_1111)),
        SequenceCase::preserving_flags("immediate definition forwards through register copies").initial_registers(&REGISTERS)
            .step(Checkpoint::preserving_flags(&[0xb8, 42, 0, 0, 0]).register(Gpr32::Eax, 42))
            .step(Checkpoint::preserving_flags(&[0x89, 0xc1]).register(Gpr32::Ecx, 42))
            .step(Checkpoint::preserving_flags(&[0x8b, 0xd1]).register(Gpr32::Edx, 42))
            .step(Checkpoint::preserving_flags(&[0x89, 0xd3]).register(Gpr32::Ebx, 42)),
        SequenceCase::preserving_flags("earlier copy survives replacing its source register").initial_registers(&REGISTERS)
            .step(Checkpoint::preserving_flags(&[0x89, 0xc1]).register(Gpr32::Ecx, 0x1111_1111))
            .step(Checkpoint::preserving_flags(&[0xb8, 9, 0, 0, 0]).register(Gpr32::Eax, 9))
            .step(Checkpoint::preserving_flags(&[0x8b, 0xd1]).register(Gpr32::Edx, 0x1111_1111)),
    ]
}

#[rustfmt::skip]
fn successful_fetch_boundaries() -> Vec<Case> {
    vec![
        Case::preserving_flags("complete register MOV does not fetch a following page", &[0x89, 0xc1])
            .at(0x1ffe).initial_registers(&REGISTERS).expect_register(Gpr32::Ecx, Exact(0x1111_1111)),
        Case::preserving_flags("ModRM is in a nonadjacent physical frame", &[0x8b, 0xf8])
            .at(0x1fff).map_page(1, 0x3000, ReadOnly).map_page(2, 0x1000, ReadOnly)
            .initial_registers(&REGISTERS).expect_register(Gpr32::Edi, Exact(0x1111_1111)),
        Case::preserving_flags("register MOV crosses wrapped EIP", &[0x89, 0xc1])
            .at(0xffff_ffff).map_page(0xfffff, 0x3000, ReadOnly).map_page(0, 0x1000, ReadOnly)
            .initial_registers(&REGISTERS).expect_register(Gpr32::Ecx, Exact(0x1111_1111)),
        Case::preserving_flags("immediate crosses contiguous physical frames", &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .at(0x1ffd).map_page(1, 0x3000, ReadOnly).map_page(2, 0x4000, ReadOnly)
            .initial_registers(&REGISTERS).expect_register(Gpr32::Eax, Exact(0x1234_5678)),
        Case::preserving_flags("immediate crosses scattered physical frames", &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .at(0x1ffd).map_page(1, 0x3000, ReadOnly).map_page(2, 0x1000, ReadOnly)
            .initial_registers(&REGISTERS).expect_register(Gpr32::Eax, Exact(0x1234_5678)),
        Case::preserving_flags("opcode and complete immediate occupy separate frames", &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .at(0x1fff).map_page(1, 0x3000, ReadOnly).map_page(2, 0x1000, ReadOnly)
            .initial_registers(&REGISTERS).expect_register(Gpr32::Eax, Exact(0x1234_5678)),
        Case::preserving_flags("immediate is complete at the page end", &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .at(0x1ffb).initial_registers(&REGISTERS).expect_register(Gpr32::Eax, Exact(0x1234_5678)),
        Case::preserving_flags("immediate MOV crosses wrapped EIP", &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .at(0xffff_fffd).map_page(0xfffff, 0x3000, ReadOnly).map_page(0, 0x1000, ReadOnly)
            .initial_registers(&REGISTERS).expect_register(Gpr32::Eax, Exact(0x1234_5678)),
        Case::preserving_flags("MOV dispatches an EIP with the sign bit set", &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .at(0x7fff_fffd).map_page(0x7ffff, 0x3000, ReadOnly).map_page(0x80000, 0x4000, ReadOnly)
            .initial_registers(&REGISTERS).expect_register(Gpr32::Eax, Exact(0x1234_5678)),
        Case::preserving_flags("one step retires one instruction even when a successor is present", &[0xb8, 42, 0, 0, 0])
            .map_page(1, 0x3000, ReadOnly).backing(0x3005, &[0xbf, 7, 0, 0, 0])
            .initial_registers(&REGISTERS).expect_register(Gpr32::Eax, Exact(42)),
    ]
}

test_cases!(register_and_immediate_moves, register_moves());
test_cases!(
    scattered_wrapping_and_complete_fetches,
    successful_fetch_boundaries()
);
test_sequences!(register_value_forwarding, forwarded_values());

#[test]
fn missing_successor_fields_preserve_completed_instruction_progress() {
    for (name, start, code, completed_eip) in [
        (
            "missing ModRM",
            0x1ffa,
            &[0xb8, 42, 0, 0, 0, 0x89][..],
            0x1fff,
        ),
        (
            "missing displacement",
            0x1ff9,
            &[0xb8, 42, 0, 0, 0, 0x89, 0x05][..],
            0x1ffe,
        ),
    ] {
        let image = Image {
            cpu: state(start),
            guest: vec![(0x3000 + (start & 0xfff), code.to_vec())],
            machine: vec![(4, vec![1, 0x30, 0, 0])],
        };
        let mut completed = image.cpu;
        completed.registers.eax = 42;
        completed.eip = completed_eip;
        completed.instruction_count = 0;
        check(
            TestModule::interpreter(),
            name,
            &image,
            &[
                Step {
                    cpu: completed,
                    ram: &[],
                    exit: Exit::Dispatch(completed_eip),
                },
                Step {
                    cpu: completed,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x2000,
                        error: 0x10,
                    },
                },
            ],
        );
    }
}

#[test]
fn missing_instruction_bytes_and_unsupported_opcodes_preserve_entry_state() {
    for (name, start, guest, machine, exit) in [
        (
            "absent opcode",
            0x1234,
            vec![],
            vec![(4, vec![0, 0xf0, 0xff, 0xff])],
            Exit::PageFault {
                address: 0x1234,
                error: 0x10,
            },
        ),
        (
            "missing first immediate byte",
            0x1fff,
            vec![(0x3fff, vec![0xb8])],
            vec![(4, vec![1, 0x30, 0, 0])],
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "partial immediate",
            0x1ffd,
            vec![(0x3ffd, vec![0xb8, 0x78, 0x56])],
            vec![(4, vec![1, 0x30, 0, 0])],
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "unsupported opcode at page end",
            0x1fff,
            vec![(0x3fff, vec![0x62])],
            vec![(4, vec![1, 0x30, 0, 0])],
            Exit::Other(0x0008_0062_0000_1fff),
        ),
        (
            "address prefix needs the next code page",
            0x1fff,
            vec![(0x3fff, vec![0x67])],
            vec![(4, vec![1, 0x30, 0, 0])],
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "missing page after EIP wrap",
            0xffff_fffd,
            vec![(0x3ffd, vec![0xb8, 0x78, 0x56]), (0x1000, vec![0x34, 0x12])],
            vec![(0x003f_fffc, vec![1, 0x30, 0, 0])],
            Exit::PageFault {
                address: 0,
                error: 0x10,
            },
        ),
    ] {
        let image = Image {
            cpu: state(start),
            guest,
            machine,
        };
        check(
            TestModule::interpreter(),
            name,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        );
    }
}

#[test]
fn interpreter_reads_live_bytes_while_a_snapshot_keeps_its_compiled_instruction() {
    let image = Image {
        cpu: state(0x1000),
        guest: vec![(0x3000, vec![0xbf, 7, 0, 0, 0])],
        machine: vec![(4, vec![1, 0x30, 0, 0])],
    };
    let snapshot =
        TestModule::new(&compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1).unwrap());
    let mut live = image.cpu;
    live.registers.edi = 7;
    live.eip = 0x1005;
    live.instruction_count = 0;
    check(
        TestModule::interpreter(),
        "live MOV EDI,7",
        &image,
        &[Step {
            cpu: live,
            ram: &[],
            exit: Exit::Dispatch(0x1005),
        }],
    );
    let mut compiled = image.cpu;
    compiled.registers.eax = 42;
    compiled.eip = 0x1005;
    compiled.instruction_count = 0;
    check(
        &snapshot,
        "snapshot MOV EAX,42",
        &image,
        &[Step {
            cpu: compiled,
            ram: &[],
            exit: Exit::Dispatch(0x1005),
        }],
    );
}
