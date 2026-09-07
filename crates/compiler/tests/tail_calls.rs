use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasm86_compiler::{
    BuildError, Func, FunctionImport, Mem, MemoryImport, Program, Signature, Type, I1, I16, I32,
    I64, I8,
};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

fn signature(parameters: &[Type], result: Type) -> Signature {
    Signature {
        parameters: parameters.to_vec(),
        result,
    }
}

fn callback(program: &mut Program, name: &str, parameters: &[Type], result: Type) -> Func {
    program.import_function(FunctionImport {
        module: "test".into(),
        name: name.into(),
        signature: signature(parameters, result),
    })
}

fn memory(program: &mut Program) -> Mem {
    program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    })
}

fn zero_arguments() -> Vec<u8> {
    let mut program = Program::new();
    let target = callback(&mut program, "receive", &[], Type::I64);
    let run = program.declare(signature(&[], Type::I64));
    program.define(run).unwrap().tail_call(target, &[]).unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn shared_arguments(store: bool, second_root: bool) -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let parameters = if second_root {
        vec![Type::I8; 3]
    } else {
        vec![Type::I8; 2]
    };
    let target = callback(&mut program, "receive", &parameters, Type::I64);
    let run = program.declare(signature(&[Type::I8], Type::I64));
    let mut body = program.define(run).unwrap();
    let raw = body.parameter::<I8>(0).unwrap().add(1);
    let raw = body.value(&raw).unwrap();
    let other = raw.add(1);
    if store {
        body.store(state, 0, &raw).unwrap();
    }
    let mut arguments = vec![raw.argument()];
    if second_root {
        arguments.push(other.argument());
    }
    arguments.push(raw.argument());
    body.tail_call(target, &arguments).unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn mixed_arguments() -> Vec<u8> {
    let mut program = Program::new();
    let target = callback(
        &mut program,
        "receive",
        &[Type::I16, Type::I1, Type::I64, Type::I8, Type::I32],
        Type::I64,
    );
    let run = program.declare(signature(
        &[Type::I1, Type::I8, Type::I16, Type::I32, Type::I64],
        Type::I64,
    ));
    let body = program.define(run).unwrap();
    let bit = body.parameter::<I1>(0).unwrap().add(1);
    let byte = body.parameter::<I8>(1).unwrap().add(1);
    let half = body.parameter::<I16>(2).unwrap().add(1);
    let word = body.parameter::<I32>(3).unwrap().add(1);
    let wide = body.parameter::<I64>(4).unwrap().add(1);
    body.tail_call(
        target,
        &[
            half.argument(),
            bit.argument(),
            wide.argument(),
            byte.argument(),
            word.argument(),
        ],
    )
    .unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn canonical_arguments() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let target = callback(
        &mut program,
        "receive",
        &[Type::I8, Type::I16, Type::I1],
        Type::I8,
    );
    let run = program.declare(signature(&[Type::I16], Type::I8));
    let mut body = program.define(run).unwrap();
    let loaded = body.load::<I8>(state, 0).unwrap();
    let parameter = body.parameter::<I16>(0).unwrap();
    body.tail_call(
        target,
        &[loaded.argument(), parameter.argument(), true.into()],
    )
    .unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn trapping_argument() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let target = callback(&mut program, "receive", &[Type::I32], Type::I64);
    let run = program.declare(signature(&[], Type::I64));
    let mut body = program.define(run).unwrap();
    let loaded = body.load::<I32>(state, 65536).unwrap();
    body.store::<I32>(state, 0, 9).unwrap();
    body.tail_call(target, &[loaded.argument()]).unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn imported_and_defined_targets() -> Vec<u8> {
    let mut program = Program::new();
    callback(&mut program, "unused", &[], Type::I1);
    let run = program.declare(signature(&[], Type::I64));
    let right = callback(&mut program, "right", &[Type::I32], Type::I64);
    let state = memory(&mut program);
    let helper = program.declare(signature(&[Type::I32], Type::I64));
    let left = callback(&mut program, "left", &[Type::I32], Type::I64);
    let direct = callback(&mut program, "direct", &[Type::I64], Type::I64);
    let other = program.declare(signature(&[], Type::I64));
    let mut body = program.define(run).unwrap();
    body.store::<I32>(state, 0, 11).unwrap();
    body.tail_call(helper, &[11.into()]).unwrap();
    let body = program.define(helper).unwrap();
    let parameter = body.parameter::<I32>(0).unwrap();
    body.tail_call(left, &[parameter.argument()]).unwrap();
    let body = program.define(other).unwrap();
    body.tail_call(right, &[22.into()]).unwrap();
    for (name, function) in [
        ("run", run),
        ("helper", helper),
        ("other", other),
        ("direct", direct),
    ] {
        program.export(name, function).unwrap();
    }
    program.compile().unwrap()
}

#[derive(Default)]
struct Code {
    locals: u32,
    writes: usize,
    adds: usize,
    masks: usize,
    tails: Vec<u32>,
    types: Vec<(Vec<ValType>, Vec<ValType>)>,
    imports: Vec<(String, TypeRef)>,
    functions: Vec<u32>,
    exports: Vec<(String, u32)>,
    store_memories: Vec<u32>,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::TypeSection(types) => {
                for ty in types.into_iter_err_on_gc_types() {
                    let ty = ty.unwrap();
                    code.types
                        .push((ty.params().to_vec(), ty.results().to_vec()));
                }
            }
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.unwrap();
                    code.imports.push((import.name.into(), import.ty));
                }
            }
            Payload::FunctionSection(functions) => code
                .functions
                .extend(functions.into_iter().map(Result::unwrap)),
            Payload::ExportSection(exports) => {
                for export in exports {
                    let export = export.unwrap();
                    assert_eq!(export.kind, ExternalKind::Func);
                    code.exports.push((export.name.into(), export.index));
                }
            }
            Payload::CodeSectionEntry(body) => {
                for local in body.get_locals_reader().unwrap() {
                    code.locals += local.unwrap().0;
                }
                let mut operators = body.get_operators_reader().unwrap();
                while !operators.eof() {
                    match operators.read().unwrap() {
                        Operator::I32Add | Operator::I64Add => code.adds += 1,
                        Operator::I32And => code.masks += 1,
                        Operator::LocalSet { .. } | Operator::LocalTee { .. } => code.writes += 1,
                        Operator::ReturnCall { function_index } => code.tails.push(function_index),
                        Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                            code.store_memories.push(memarg.memory)
                        }
                        Operator::Call { .. } | Operator::Return => {
                            panic!("a tail call must use return_call")
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    code
}

#[test]
fn a_tail_call_can_have_no_arguments() {
    let code = inspect(&zero_arguments());
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (0, 0, 0, 0)
    );
    assert_eq!(code.tails.len(), 1);
}

#[test]
fn duplicate_narrow_arguments_share_arithmetic_and_normalization() {
    let code = inspect(&shared_arguments(false, false));
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (1, 1, 1, 1)
    );
    assert_eq!(code.tails.len(), 1);
}

#[test]
fn stores_share_raw_values_with_normalized_arguments() {
    let code = inspect(&shared_arguments(true, false));
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (1, 1, 1, 2)
    );
    let code = inspect(&shared_arguments(true, true));
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (2, 2, 2, 2)
    );
}

#[test]
fn canonical_arguments_need_no_masks() {
    let code = inspect(&canonical_arguments());
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (0, 0, 0, 0)
    );
    assert_eq!(code.tails.len(), 1);
}

#[test]
fn function_imports_remap_calls_and_exports_without_shifting_memories() {
    let code = inspect(&imported_and_defined_targets());
    let imports: Vec<_> = code
        .imports
        .iter()
        .filter_map(|(name, ty)| match ty {
            TypeRef::Func(index) => Some((name.as_str(), *index)),
            _ => None,
        })
        .collect();
    assert_eq!(
        imports.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["right", "left", "direct"]
    );
    assert_eq!(
        code.types,
        [
            (vec![], vec![ValType::I64]),
            (vec![ValType::I32], vec![ValType::I64]),
            (vec![ValType::I64], vec![ValType::I64])
        ]
    );
    assert_eq!(code.functions, [0, 1, 0]);
    assert_eq!(imports[0].1, code.functions[1]);
    assert_eq!(imports[1].1, code.functions[1]);
    assert_eq!(imports[2].1, 2);
    let index = |name| {
        code.exports
            .iter()
            .find(|(export, _)| export == name)
            .unwrap()
            .1
    };
    assert_eq!(code.tails, [index("helper"), 1, 0]);
    assert_eq!(index("direct"), 2);
    assert_eq!((index("run"), index("other")), (3, 5));
    assert_eq!(code.store_memories, [0]);
}

#[test]
fn tail_signatures_require_logical_argument_and_result_types() {
    for (target_parameter, target_result) in [(Type::I8, Type::I1), (Type::I1, Type::I8)] {
        let mut program = Program::new();
        let target = callback(&mut program, "receive", &[target_parameter], target_result);
        let run = program.declare(signature(&[], Type::I1));
        let body = program.define(run).unwrap();
        let bit = body.value::<I1>(true).unwrap();
        assert!(matches!(
            body.tail_call(target, &[bit.argument()]),
            Err(BuildError::TypeMismatch { .. })
        ));
        let body = program.define(run).unwrap();
        body.return_(false).unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|part| !matches!(part.unwrap(), Payload::ImportSection(_))));
    }
}

struct ModuleFile(PathBuf);
impl ModuleFile {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-tail-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }
}
impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn check(flags: &[&str], bytes: &[u8], arguments: &[&str], expected: &str) {
    let module = ModuleFile::new(bytes);
    let output = Command::new("node")
        .args(flags)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/execute-tail.mjs"
        ))
        .arg(&module.0)
        .args(arguments)
        .output()
        .expect("the explicit V8 lane requires Node.js on PATH");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        expected,
        "args {arguments:?}, V8 flags {flags:?}"
    );
}

fn check_execution(flags: &[&str]) {
    check(
        flags,
        &zero_arguments(),
        &["run", "-", "receive:i64:9223372036854775807"],
        "receive()\nreturn 9223372036854775807\n",
    );
    check(
        flags,
        &shared_arguments(false, false),
        &["run", "-", "receive:i64:1234567890123456789", "i32:255"],
        "receive(0,0)\nreturn 1234567890123456789\n",
    );
    check(
        flags,
        &shared_arguments(true, false),
        &[
            "run",
            "ffa55a",
            "receive:i64:-9223372036854775808",
            "i32:255",
        ],
        "receive(0,0) 00a55a\nreturn -9223372036854775808\nstate 00a55a\n",
    );
    check(
        flags,
        &shared_arguments(true, true),
        &["run", "ffa55a", "receive:i64:17", "i32:255"],
        "receive(0,1,0) 00a55a\nreturn 17\nstate 00a55a\n",
    );
    let mixed = mixed_arguments();
    check(
        flags,
        &mixed,
        &[
            "run",
            "-",
            "receive:i64:41",
            "i32:1",
            "i32:255",
            "i32:65535",
            "i32:2147483647",
            "i64:9223372036854775807",
        ],
        "receive(0,0,-9223372036854775808,0,-2147483648)\nreturn 41\n",
    );
    check(
        flags,
        &mixed,
        &[
            "run",
            "-",
            "receive:i64:-1",
            "i32:0",
            "i32:7",
            "i32:9",
            "i32:19",
            "i64:41",
        ],
        "receive(10,1,42,8,20)\nreturn -1\n",
    );
    check(
        flags,
        &canonical_arguments(),
        &["run", "ffa55a", "receive:i32:255", "i32:65535"],
        "receive(255,65535,1) ffa55a\nreturn 255\nstate ffa55a\n",
    );
    check(
        flags,
        &trapping_argument(),
        &["run", "07000000", "receive:i64:99"],
        "return trap\nstate 09000000\n",
    );
    let bindings = imported_and_defined_targets();
    let callbacks = "right:i64:101,left:i64:202,direct:i64:-9223372036854775808";
    check(
        flags,
        &bindings,
        &["run", "07000000", callbacks],
        "left(11) 0b000000\nreturn 202\nstate 0b000000\n",
    );
    check(
        flags,
        &bindings,
        &["other", "07000000", callbacks],
        "right(22) 07000000\nreturn 101\nstate 07000000\n",
    );
    check(
        flags,
        &bindings,
        &["direct", "07000000", callbacks, "i64:9223372036854775807"],
        "direct(9223372036854775807) 07000000\nreturn -9223372036854775808\nstate 07000000\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn tail_calls_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn tail_calls_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
