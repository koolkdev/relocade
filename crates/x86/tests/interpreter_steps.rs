use std::{
    fmt::Write as _,
    fs,
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
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

fn state(eip: u32) -> [u8; 152] {
    let mut bytes = [0xa5; 152];
    for (offset, value) in [
        (24, 0x1111_1111u32),
        (28, 0x2222_2222),
        (32, 0x3333_3333),
        (36, 0x4444_4444),
        (40, 0x5555_5555),
        (44, 0x6666_6666),
        (48, 0x7777_7777),
        (52, 0x8888_8888),
        (56, eip),
        (144, 0xffff_ffff),
        (148, 0),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

struct Image<'a> {
    label: &'static str,
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

struct ModuleFile {
    path: PathBuf,
    entry: String,
}

impl ModuleFile {
    fn new(module: &CompiledModule) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-step-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, &module.bytes).unwrap();
        Self {
            path,
            entry: module.entry.clone(),
        }
    }

    fn check(&self, flags: &[&str], image: &Image<'_>, updates: &[(usize, u32)], outcome: Outcome) {
        let mut expected = image.cpu;
        for (offset, value) in updates {
            expected[*offset..*offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let expected_state = hex(&expected);
        let terminal = match outcome {
            Outcome::Dispatch(eip) => {
                format!("dispatch({eip}) {expected_state}\nreturn -9223372036854775808\n")
            }
            Outcome::Exit(word) => format!("return {word}\n"),
            Outcome::Trap => "return trap\n".into(),
        };
        let expected =
            format!("{terminal}state {expected_state}\nguest unchanged\nmachine unchanged\n");
        let mut child = Command::new("node")
            .args(flags)
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/execute-step.mjs"
            ))
            .arg(&self.path)
            .arg(&self.entry)
            .arg(i64::MIN.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the explicit V8 lane requires Node.js on PATH");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(image.input().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "{}, entry {}, updates {updates:?}, flags {flags:?}",
            image.label,
            self.entry
        );
    }
}

impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[test]
fn live_step_exposes_the_cpu_ram_page_map_and_dispatch_abi() {
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
fn the_fast_path_loads_one_immediate_and_publishes_one_indexed_register() {
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
    assert_eq!(fast.guest_loads, [(0, 8), (1, 32)]);
    let dispatch = dispatch.unwrap();
    assert_eq!(
        fast.tails
            .iter()
            .filter(|&&target| target == dispatch)
            .count(),
        1
    );
    assert_eq!(
        fast.tails
            .iter()
            .filter(|&&target| target >= imported_functions)
            .count(),
        1
    );
    for body in bodies {
        assert_eq!(body.cpu_loads, [56, 144]);
        assert_eq!(body.stores, [(0, 24), (0, 56), (0, 144)]);
        assert!(body.tails.contains(&dispatch));
    }
}

fn check_execution(flags: &[&str]) {
    let step = ModuleFile::new(&compile_interpreter_step().unwrap());
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
