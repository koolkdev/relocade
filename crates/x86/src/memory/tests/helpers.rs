use std::collections::{BTreeMap, BTreeSet};

use wasmparser::{ExternalKind, FuncType, ValType};

use super::*;

struct Function {
    signature: FuncType,
    calls: BTreeSet<u32>,
    byte_reads: u32,
    byte_writes: u32,
}

struct Module {
    functions: Vec<Function>,
    exports: BTreeMap<String, u32>,
}

impl Module {
    fn inspect(bytes: &[u8]) -> Self {
        Validator::new().validate_all(bytes).unwrap();
        let mut types = Vec::new();
        let mut function_types = Vec::new();
        let mut functions = Vec::new();
        let mut exports = BTreeMap::new();
        let mut guest_index = None;
        let mut memories = 0;
        for payload in Parser::new(0).parse_all(bytes) {
            match payload.unwrap() {
                Payload::TypeSection(section) => {
                    types.extend(section.into_iter_err_on_gc_types().map(Result::unwrap));
                }
                Payload::ImportSection(section) => {
                    for import in section {
                        let import = import.unwrap();
                        if matches!(import.ty, TypeRef::Memory(_)) {
                            if import.module == "wasm86" && import.name == "guest" {
                                guest_index = Some(memories);
                            }
                            memories += 1;
                        } else {
                            panic!("these memory fixtures have no function imports");
                        }
                    }
                }
                Payload::FunctionSection(section) => {
                    function_types.extend(section.into_iter().map(Result::unwrap));
                }
                Payload::ExportSection(section) => {
                    for export in section {
                        let export = export.unwrap();
                        if export.kind == ExternalKind::Func {
                            exports.insert(export.name.to_owned(), export.index);
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    let mut function = Function {
                        signature: types[function_types[functions.len()] as usize].clone(),
                        calls: BTreeSet::new(),
                        byte_reads: 0,
                        byte_writes: 0,
                    };
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::Call { function_index } => {
                                function.calls.insert(function_index);
                            }
                            Operator::I32Load8U { memarg }
                                if Some(memarg.memory) == guest_index =>
                            {
                                function.byte_reads += 1;
                            }
                            Operator::I32Store8 { memarg }
                                if Some(memarg.memory) == guest_index =>
                            {
                                function.byte_writes += 1;
                            }
                            _ => {}
                        }
                    }
                    functions.push(function);
                }
                _ => {}
            }
        }
        Self { functions, exports }
    }

    fn transfer_helpers(&self, writes: bool) -> Vec<(u32, &Function)> {
        self.functions
            .iter()
            .enumerate()
            .filter(|(index, function)| {
                !self.exports.values().any(|export| *export == *index as u32)
                    && if writes {
                        function.byte_writes != 0
                    } else {
                        function.byte_reads != 0
                    }
            })
            .map(|(index, function)| (index as u32, function))
            .collect()
    }
}

fn check_reuse<T: MemoryInt>()
where
    I64: AtLeast<T>,
{
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    for name in ["read_a", "read_b"] {
        define_read::<T>(&mut program, &memory, name);
    }
    for name in ["write_a", "write_b"] {
        define_write::<T>(&mut program, &memory, name);
    }
    let module = Module::inspect(&program.compile().unwrap());
    for (writes, names) in [
        (false, ["read_a", "read_b"]),
        (true, ["write_a", "write_b"]),
    ] {
        let helpers = module.transfer_helpers(writes);
        assert_eq!(
            helpers.len(),
            1,
            "one shared helper for the requested width"
        );
        let (helper_index, helper) = helpers[0];
        assert_eq!(
            if writes {
                helper.byte_writes
            } else {
                helper.byte_reads
            },
            T::BYTES,
            "the helper transfers exactly the requested width"
        );
        let carrier = if T::TYPE == Type::I64 {
            ValType::I64
        } else {
            ValType::I32
        };
        if writes {
            assert_eq!(helper.signature.params(), [ValType::I32, carrier]);
            assert!(helper.signature.results().is_empty());
        } else {
            assert_eq!(helper.signature.params(), [ValType::I32]);
            assert_eq!(helper.signature.results(), [carrier]);
        }
        for name in names {
            let caller = &module.functions[module.exports[name] as usize];
            assert!(
                caller.calls.contains(&helper_index),
                "{name} shares the helper"
            );
            assert_eq!((caller.byte_reads, caller.byte_writes), (0, 0));
        }
    }
}

#[test]
fn scattered_helpers_are_shared_across_functions_at_each_requested_width() {
    check_reuse::<I16>();
    check_reuse::<I32>();
    check_reuse::<I64>();
}

#[test]
fn read_only_accesses_do_not_build_writers_or_other_widths() {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    define_read::<I16>(&mut program, &memory, "read");
    let module = Module::inspect(&program.compile().unwrap());
    let readers = module.transfer_helpers(false);
    assert_eq!(readers.len(), 1);
    assert_eq!(readers[0].1.byte_reads, 2);
    assert!(module.transfer_helpers(true).is_empty());
}

#[test]
fn byte_accesses_do_not_build_transfer_helpers() {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    define_read::<I8>(&mut program, &memory, "read");
    define_write::<I8>(&mut program, &memory, "write");
    let module = Module::inspect(&program.compile().unwrap());
    assert!(module.transfer_helpers(false).is_empty());
    assert!(module.transfer_helpers(true).is_empty());
}

#[test]
fn shared_readers_preserve_old_and_new_values_across_a_scattered_write_in_wasmtime() {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I64],
                results: vec![Type::I64],
            },
            |mut body| {
                let address = body.parameter::<I32>(0)?;
                let replacement = body.parameter::<I64>(1)?;
                let access = memory.resolve_access(
                    &mut body,
                    &address,
                    I64::BYTES,
                    Intent::Write,
                    exit::exception,
                )?;
                let before = memory.read::<I64>(&mut body, &access, 0)?;
                memory.write(&mut body, &access, 0, &replacement)?;
                let after = memory.read::<I64>(&mut body, &access, 0)?;
                body.return_(
                    before
                        .eq(0x8877_6655_4433_2211u64)
                        .and(after.eq(replacement))
                        .unsigned()
                        .extend::<I64>(),
                )
            },
        )
        .unwrap();
    program.export("replace", function).unwrap();
    let module = TestModule::new(&crate::CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "replace".into(),
    });
    let case = WIDTHS.iter().find(|case| case.name == "64").unwrap();
    let input = image(
        case,
        0xa000,
        3,
        3,
        case.first,
        case.second,
        &[Argument::I32(20476), Argument::I64(72623859790382856)],
    );
    check(
        &module,
        &input,
        1,
        &[
            (0x8ffc, 8),
            (0x8ffd, 7),
            (0x8ffe, 6),
            (0x8fff, 5),
            (0xa000, 4),
            (0xa001, 3),
            (0xa002, 2),
            (0xa003, 1),
        ],
    );
}
