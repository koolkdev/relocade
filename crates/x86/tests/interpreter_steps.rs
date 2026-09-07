use std::fmt::Write as _;

#[path = "support/step.rs"]
mod step;
use step::ModuleFile;
use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, CompiledModule};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

const MOVES: [(&[u8], usize, u32); 8] = [
    (&[0xb8, 0x78, 0x56, 0x34, 0x12], 24, 0x1234_5678),
    (&[0xb9, 0, 0, 0, 0x80], 28, 0x8000_0000),
    (&[0xba, 0xff, 0xff, 0xff, 0xff], 32, 0xffff_ffff),
    (&[0xbb, 0, 0, 0, 0], 36, 0),
    (&[0xbc, 0xf3, 0x0f, 0xb8, 0x66], 40, 0x66b8_0ff3),
    (&[0xbd, 0xff, 0xff, 0xff, 0x7f], 44, 0x7fff_ffff),
    (&[0xbe, 0xef, 0xbe, 0xad, 0xde], 48, 0xdead_beef),
    (&[0xbf, 0x21, 0x43, 0x65, 0x87], 52, 0x8765_4321),
];

const REGISTERS: [(usize, u32); 8] = [
    (24, 0x1111_1111),
    (28, 0x2222_2222),
    (32, 0x3333_3333),
    (36, 0x4444_4444),
    (40, 0x5555_5555),
    (44, 0x6666_6666),
    (48, 0x7777_7777),
    (52, 0x8888_8888),
];

fn state(eip: u32) -> [u8; 152] {
    let mut bytes = [0xa5; 152];
    for (offset, value) in REGISTERS {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [(56, eip), (144, 0xffff_ffff), (148, 0)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

struct Image<'a> {
    label: &'a str,
    cpu: [u8; 152],
    guest: &'a [(u32, &'a [u8])],
    machine: &'a [(u32, &'a [u8])],
}

impl Image<'_> {
    fn input(&self) -> String {
        fn patches(patches: &[(u32, &[u8])]) -> String {
            patches
                .iter()
                .map(|(offset, bytes)| format!("[{offset},{bytes:?}]"))
                .collect::<Vec<_>>()
                .join(",")
        }
        format!(
            "[{:?},[{}],[{}]]",
            self.cpu,
            patches(self.guest),
            patches(self.machine)
        )
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").unwrap();
    }
    text
}

enum Outcome {
    Dispatch(i32),
    Exit(u64),
    Trap,
}

impl ModuleFile {
    fn check(&self, flags: &[&str], image: &Image<'_>, updates: &[(usize, u32)], outcome: Outcome) {
        self.check_steps(flags, image, &[(updates, outcome)]);
    }

    fn check_steps(&self, flags: &[&str], image: &Image<'_>, steps: &[(&[(usize, u32)], Outcome)]) {
        let mut cpu = image.cpu;
        let mut expected = String::new();
        for (updates, outcome) in steps {
            for &(offset, value) in *updates {
                cpu[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            let state = hex(&cpu);
            match outcome {
                Outcome::Dispatch(eip) => {
                    writeln!(&mut expected, "dispatch({eip}) {state}").unwrap();
                    expected.push_str("return -9223372036854775808\n");
                }
                Outcome::Exit(word) => writeln!(&mut expected, "return {word}").unwrap(),
                Outcome::Trap => expected.push_str("return trap\n"),
            }
            writeln!(&mut expected, "state {state}").unwrap();
        }
        expected.push_str("guest unchanged\nmachine unchanged\n");
        assert_eq!(
            self.observe(flags, &image.input(), steps.len()),
            expected,
            "{}, entry {}, flags {flags:?}",
            image.label,
            self.entry
        );
    }
}

#[test]
fn interpreter_step_exposes_the_cpu_ram_page_map_and_dispatch_abi() {
    let module = compile_interpreter_step().unwrap();
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
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].0, "dispatch");
    assert_eq!(
        types[imports[0].1 as usize],
        (vec![ValType::I32], vec![ValType::I64])
    );
    let entry = exported.unwrap() as usize - imports.len();
    assert_eq!(
        types[functions[entry] as usize],
        (vec![], vec![ValType::I64])
    );
}

#[test]
fn runtime_decoding_keeps_the_wide_immediate_and_publishes_each_completed_path() {
    #[derive(Default)]
    struct Code {
        cpu_loads: Vec<u64>,
        guest_loads: Vec<(u64, u8)>,
        stores: Vec<(u32, u64)>,
        tails: Vec<u32>,
    }
    let module = compile_interpreter_step().unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let mut bodies = Vec::new();
    let mut imported_functions = 0;
    let mut dispatch = None;
    let mut step = None;
    for payload in Parser::new(0).parse_all(&module.bytes) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                for import in section {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Func(_)) {
                        if import.name == "dispatch" {
                            dispatch = Some(imported_functions);
                        }
                        imported_functions += 1;
                    }
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.name == "step" {
                        step = Some(export.index - imported_functions);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut code = Code::default();
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::I32Load { memarg } if memarg.memory == 0 => {
                            code.cpu_loads.push(memarg.offset)
                        }
                        Operator::I32Load { memarg } if memarg.memory == 1 => {
                            code.guest_loads.push((memarg.offset, 32))
                        }
                        Operator::I32Load8U { memarg } if memarg.memory == 1 => {
                            code.guest_loads.push((memarg.offset, 8))
                        }
                        Operator::I32Store { memarg } => {
                            code.stores.push((memarg.memory, memarg.offset))
                        }
                        Operator::ReturnCall { function_index } => code.tails.push(function_index),
                        _ => {}
                    }
                }
                bodies.push(code);
            }
            _ => {}
        }
    }
    let fast = &bodies[step.unwrap() as usize];
    assert_eq!(
        fast.guest_loads
            .iter()
            .filter(|&&(_, bits)| bits == 32)
            .copied()
            .collect::<Vec<_>>(),
        [(1, 32)]
    );
    assert!(fast.guest_loads.contains(&(0, 8)));
    assert!(!fast.cpu_loads.contains(&24));
    assert!(bodies.iter().any(|body| body.cpu_loads.contains(&24)));
    assert!(bodies.iter().any(|body| body.guest_loads.contains(&(1, 8))));
    let dispatch = dispatch.unwrap();
    assert!(fast
        .tails
        .iter()
        .any(|&target| target >= imported_functions));
    assert_eq!(
        fast.cpu_loads
            .iter()
            .filter(|&&offset| offset == 56)
            .count(),
        1
    );
    for body in bodies {
        let exits = body
            .tails
            .iter()
            .filter(|&&target| target == dispatch)
            .count();
        assert!(exits > 0);

        assert_eq!(
            body.cpu_loads
                .iter()
                .filter(|&&offset| offset == 144)
                .count(),
            exits
        );
        for offset in [24, 56, 144] {
            assert_eq!(
                body.stores
                    .iter()
                    .filter(|&&access| access == (0, offset))
                    .count(),
                exits
            );
        }
        assert_eq!(body.stores.len(), exits * 3);
    }
}

fn check_register_moves(flags: &[&str], step: &ModuleFile) {
    for (source, &(_, value)) in REGISTERS.iter().enumerate() {
        for (destination, &(offset, _)) in REGISTERS.iter().enumerate() {
            for bytes in [
                [0x89, 0xc0 | ((source as u8) << 3) | destination as u8],
                [0x8b, 0xc0 | ((destination as u8) << 3) | source as u8],
            ] {
                let label = format!(
                    "opcode {:02x}, source {source}, destination {destination}",
                    bytes[0]
                );
                let image = Image {
                    label: &label,
                    cpu: state(0x1234),
                    guest: &[(0x3234, &bytes)],
                    machine: &[(4, &[1, 0x30, 0, 0])],
                };
                let updates = [(offset, value), (56, 0x1236), (144, 0)];
                step.check(flags, &image, &updates, Outcome::Dispatch(4662));
                let snapshot =
                    ModuleFile::new(&compile_block_from_bytes(0x1234, &bytes, 1).unwrap());
                snapshot.check(flags, &image, &updates, Outcome::Dispatch(4662));
            }
        }
    }

    let rotation = &[
        0x89, 0xc2, 0x8b, 0xc1, 0x89, 0xd1, 0x8b, 0xf8, 0x89, 0xce, 0x8b, 0xda,
    ];
    let image = Image {
        label: "register rotation retains source values",
        cpu: state(0x1000),
        guest: &[(0x3000, rotation)],
        machine: &[(4, &[1, 0x30, 0, 0])],
    };
    step.check_steps(
        flags,
        &image,
        &[
            (
                &[(32, 0x1111_1111), (56, 0x1002), (144, 0)],
                Outcome::Dispatch(4098),
            ),
            (
                &[(24, 0x2222_2222), (56, 0x1004), (144, 1)],
                Outcome::Dispatch(4100),
            ),
            (
                &[(28, 0x1111_1111), (56, 0x1006), (144, 2)],
                Outcome::Dispatch(4102),
            ),
            (
                &[(52, 0x2222_2222), (56, 0x1008), (144, 3)],
                Outcome::Dispatch(4104),
            ),
            (
                &[(48, 0x1111_1111), (56, 0x100a), (144, 4)],
                Outcome::Dispatch(4106),
            ),
            (
                &[(36, 0x1111_1111), (56, 0x100c), (144, 5)],
                Outcome::Dispatch(4108),
            ),
        ],
    );
    let snapshot = ModuleFile::new(&compile_block_from_bytes(0x1000, rotation, 6).unwrap());
    snapshot.check(
        flags,
        &image,
        &[
            (24, 0x2222_2222),
            (28, 0x1111_1111),
            (32, 0x1111_1111),
            (36, 0x1111_1111),
            (48, 0x1111_1111),
            (52, 0x2222_2222),
            (56, 0x100c),
            (144, 5),
        ],
        Outcome::Dispatch(4108),
    );

    let forward = &[0xb8, 42, 0, 0, 0, 0x89, 0xc1, 0x8b, 0xd1, 0x89, 0xd3];
    let image = Image {
        label: "immediate definition forwards through register copies",
        cpu: state(0x1000),
        guest: &[(0x3000, forward)],
        machine: image.machine,
    };
    step.check_steps(
        flags,
        &image,
        &[
            (&[(24, 42), (56, 0x1005), (144, 0)], Outcome::Dispatch(4101)),
            (&[(28, 42), (56, 0x1007), (144, 1)], Outcome::Dispatch(4103)),
            (&[(32, 42), (56, 0x1009), (144, 2)], Outcome::Dispatch(4105)),
            (&[(36, 42), (56, 0x100b), (144, 3)], Outcome::Dispatch(4107)),
        ],
    );
    let snapshot = ModuleFile::new(&compile_block_from_bytes(0x1000, forward, 4).unwrap());
    snapshot.check(
        flags,
        &image,
        &[
            (24, 42),
            (28, 42),
            (32, 42),
            (36, 42),
            (56, 0x100b),
            (144, 3),
        ],
        Outcome::Dispatch(4107),
    );

    let old_value = &[0x89, 0xc1, 0xb8, 9, 0, 0, 0, 0x8b, 0xd1];
    let image = Image {
        label: "earlier copy survives replacing its source register",
        cpu: state(0x1000),
        guest: &[(0x3000, old_value)],
        machine: image.machine,
    };
    step.check_steps(
        flags,
        &image,
        &[
            (
                &[(28, 0x1111_1111), (56, 0x1002), (144, 0)],
                Outcome::Dispatch(4098),
            ),
            (&[(24, 9), (56, 0x1007), (144, 1)], Outcome::Dispatch(4103)),
            (
                &[(32, 0x1111_1111), (56, 0x1009), (144, 2)],
                Outcome::Dispatch(4105),
            ),
        ],
    );
    let snapshot = ModuleFile::new(&compile_block_from_bytes(0x1000, old_value, 3).unwrap());
    snapshot.check(
        flags,
        &image,
        &[
            (24, 9),
            (28, 0x1111_1111),
            (32, 0x1111_1111),
            (56, 0x1009),
            (144, 2),
        ],
        Outcome::Dispatch(4105),
    );

    let complete = Image {
        label: "complete register MOV does not fetch a following page",
        cpu: state(0x1ffe),
        guest: &[(0x3ffe, &[0x89, 0xc1])],
        machine: image.machine,
    };
    step.check(
        flags,
        &complete,
        &[(28, 0x1111_1111), (56, 0x2000), (144, 0)],
        Outcome::Dispatch(8192),
    );
    let scattered = Image {
        label: "ModRM is in a nonadjacent physical frame",
        cpu: state(0x1fff),
        guest: &[(0x3fff, &[0x8b]), (0x1000, &[0xf8])],
        machine: &[(4, &[1, 0x30, 0, 0, 1, 0x10, 0, 0])],
    };
    step.check(
        flags,
        &scattered,
        &[(52, 0x1111_1111), (56, 0x2001), (144, 0)],
        Outcome::Dispatch(8193),
    );
    let wrapped = Image {
        label: "register MOV crosses wrapped EIP",
        cpu: state(0xffff_ffff),
        guest: &[(0x3fff, &[0x89]), (0x1000, &[0xc1])],
        machine: &[(0x003f_fffc, &[1, 0x30, 0, 0]), (0, &[1, 0x10, 0, 0])],
    };
    step.check(
        flags,
        &wrapped,
        &[(28, 0x1111_1111), (56, 1), (144, 0)],
        Outcome::Dispatch(1),
    );
    let snapshot =
        ModuleFile::new(&compile_block_from_bytes(0xffff_ffff, &[0x89, 0xc1], 1).unwrap());
    snapshot.check(
        flags,
        &wrapped,
        &[(28, 0x1111_1111), (56, 1), (144, 0)],
        Outcome::Dispatch(1),
    );

    let missing_modrm = Image {
        label: "missing ModRM preserves the preceding instruction's progress",
        cpu: state(0x1ffa),
        guest: &[(0x3ffa, &[0xb8, 42, 0, 0, 0, 0x89])],
        machine: image.machine,
    };
    step.check_steps(
        flags,
        &missing_modrm,
        &[
            (&[(24, 42), (56, 0x1fff), (144, 0)], Outcome::Dispatch(8191)),
            (&[], Outcome::Exit(0x0004_0010_0000_2000)),
        ],
    );
    let memory_form = Image {
        label: "unsupported memory ModRM does not fetch its displacement",
        cpu: state(0x1ff9),
        guest: &[(0x3ff9, &[0xb8, 42, 0, 0, 0, 0x89, 0x05])],
        machine: image.machine,
    };
    step.check_steps(
        flags,
        &memory_form,
        &[
            (&[(24, 42), (56, 0x1ffe), (144, 0)], Outcome::Dispatch(8190)),
            (&[], Outcome::Exit(0x0008_0089_0000_1ffe)),
        ],
    );
}

fn check_execution(flags: &[&str]) {
    let step = ModuleFile::new(&compile_interpreter_step().unwrap());
    check_register_moves(flags, &step);
    // Virtual page 1 maps to frame 3. PRESENT alone permits instruction fetch.
    for &(bytes, offset, value) in &MOVES {
        let image = Image {
            label: "all register codes",
            cpu: state(0x1234),
            guest: &[(0x3234, bytes)],
            machine: &[(4, &[1, 0x30, 0, 0])],
        };
        let updates = [(offset, value), (56, 0x1239), (144, 0)];
        step.check(flags, &image, &updates, Outcome::Dispatch(4665));
        let snapshot = ModuleFile::new(&compile_block_from_bytes(0x1234, bytes, 1).unwrap());
        snapshot.check(flags, &image, &updates, Outcome::Dispatch(4665));
    }
    let contiguous = Image {
        label: "contiguous crossing",
        cpu: state(0x1ffd),
        guest: &[(0x3ffd, MOVES[0].0)],
        machine: &[(4, &[1, 0x30, 0, 0, 1, 0x40, 0, 0])],
    };
    step.check(
        flags,
        &contiguous,
        &[(24, 0x1234_5678), (56, 0x2002), (144, 0)],
        Outcome::Dispatch(8194),
    );
    let scattered = Image {
        label: "scattered immediate",
        cpu: state(0x1ffd),
        guest: &[(0x3ffd, &[0xb8, 0x78, 0x56]), (0x1000, &[0x34, 0x12])],
        machine: &[(4, &[1, 0x30, 0, 0, 1, 0x10, 0, 0])],
    };
    step.check(
        flags,
        &scattered,
        &[(24, 0x1234_5678), (56, 0x2002), (144, 0)],
        Outcome::Dispatch(8194),
    );
    let snapshot = ModuleFile::new(&compile_block_from_bytes(0x1ffd, MOVES[0].0, 1).unwrap());
    snapshot.check(
        flags,
        &scattered,
        &[(24, 0x1234_5678), (56, 0x2002), (144, 0)],
        Outcome::Dispatch(8194),
    );
    let split_opcode = Image {
        label: "opcode and complete immediate in separate frames",
        cpu: state(0x1fff),
        guest: &[(0x3fff, &[0xb8]), (0x1000, &[0x78, 0x56, 0x34, 0x12])],
        machine: &[(4, &[1, 0x30, 0, 0, 1, 0x10, 0, 0])],
    };
    step.check(
        flags,
        &split_opcode,
        &[(24, 0x1234_5678), (56, 0x2004), (144, 0)],
        Outcome::Dispatch(8196),
    );
    // The next page is absent; a complete instruction does not fetch beyond its five bytes.
    let exact_end = Image {
        label: "complete at page end",
        cpu: state(0x1ffb),
        guest: &[(0x3ffb, MOVES[0].0)],
        machine: &[(4, &[1, 0x30, 0, 0])],
    };
    step.check(
        flags,
        &exact_end,
        &[(24, 0x1234_5678), (56, 0x2000), (144, 0)],
        Outcome::Dispatch(8192),
    );
    let absent_opcode = Image {
        label: "absent opcode",
        cpu: state(0x1234),
        guest: &[],
        machine: &[(4, &[0, 0xf0, 0xff, 0xff])],
    };
    step.check(
        flags,
        &absent_opcode,
        &[],
        Outcome::Exit(0x0004_0010_0000_1234),
    );
    let missing_immediate = Image {
        label: "missing first immediate byte",
        cpu: state(0x1fff),
        guest: &[(0x3fff, &[0xb8])],
        machine: &[(4, &[1, 0x30, 0, 0])],
    };
    step.check(
        flags,
        &missing_immediate,
        &[],
        Outcome::Exit(0x0004_0010_0000_2000),
    );
    let partial_immediate = Image {
        label: "partial immediate",
        cpu: state(0x1ffd),
        guest: &[(0x3ffd, &[0xb8, 0x78, 0x56])],
        machine: &[(4, &[1, 0x30, 0, 0])],
    };
    step.check(
        flags,
        &partial_immediate,
        &[],
        Outcome::Exit(0x0004_0010_0000_2000),
    );
    for (opcode, exit) in [(0x90, 0x0008_0090_0000_1fff), (0x66, 0x0008_0066_0000_1fff)] {
        let unsupported = Image {
            label: "unsupported opcode at page end",
            cpu: state(0x1fff),
            guest: &[(0x3fff, &[opcode])],
            machine: &[(4, &[1, 0x30, 0, 0])],
        };
        step.check(flags, &unsupported, &[], Outcome::Exit(exit));
    }
    let wrapped = Image {
        label: "wrapped instruction",
        cpu: state(0xffff_fffd),
        guest: &[(0x3ffd, &[0xb8, 0x78, 0x56]), (0x1000, &[0x34, 0x12])],
        machine: &[(0x003f_fffc, &[1, 0x30, 0, 0]), (0, &[1, 0x10, 0, 0])],
    };
    step.check(
        flags,
        &wrapped,
        &[(24, 0x1234_5678), (56, 2), (144, 0)],
        Outcome::Dispatch(2),
    );
    let snapshot = ModuleFile::new(&compile_block_from_bytes(0xffff_fffd, MOVES[0].0, 1).unwrap());
    snapshot.check(
        flags,
        &wrapped,
        &[(24, 0x1234_5678), (56, 2), (144, 0)],
        Outcome::Dispatch(2),
    );
    let wrapped_fault = Image {
        label: "missing page after EIP wrap",
        cpu: wrapped.cpu,
        guest: wrapped.guest,
        machine: &[(0x003f_fffc, &[1, 0x30, 0, 0])],
    };
    step.check(
        flags,
        &wrapped_fault,
        &[],
        Outcome::Exit(0x0004_0010_0000_0000),
    );
    let high_eip = Image {
        label: "signed dispatch EIP",
        cpu: state(0x7fff_fffd),
        guest: &[(0x3ffd, MOVES[0].0)],
        machine: &[(0x001f_fffc, &[1, 0x30, 0, 0, 1, 0x40, 0, 0])],
    };
    step.check(
        flags,
        &high_eip,
        &[(24, 0x1234_5678), (56, 0x8000_0002), (144, 0)],
        Outcome::Dispatch(-2147483646),
    );
    let invalid_frame = Image {
        label: "present frame outside RAM",
        cpu: state(0x1000),
        guest: &[],
        machine: &[(4, &[1, 0, 1, 0])],
    };
    step.check(flags, &invalid_frame, &[], Outcome::Trap);
    let two = Image {
        label: "one instruction only",
        cpu: state(0x1000),
        guest: &[(0x3000, &[0xb8, 42, 0, 0, 0, 0xbf, 7, 0, 0, 0])],
        machine: &[(4, &[1, 0x30, 0, 0])],
    };
    step.check(
        flags,
        &two,
        &[(24, 42), (56, 0x1005), (144, 0)],
        Outcome::Dispatch(4101),
    );
    let changed = Image {
        label: "live bytes differ from snapshot",
        cpu: state(0x1000),
        guest: &[(0x3000, &[0xbf, 7, 0, 0, 0])],
        machine: &[(4, &[1, 0x30, 0, 0])],
    };
    let snapshot =
        ModuleFile::new(&compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1).unwrap());
    step.check(
        flags,
        &changed,
        &[(52, 7), (56, 0x1005), (144, 0)],
        Outcome::Dispatch(4101),
    );
    snapshot.check(
        flags,
        &changed,
        &[(24, 42), (56, 0x1005), (144, 0)],
        Outcome::Dispatch(4101),
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn interpreter_steps_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn interpreter_steps_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
