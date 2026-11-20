use super::*;
use crate::support::config::REAL16;
use wasmparser::{Parser, Payload};

fn imports(bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for import in imports {
                let import = import.unwrap();
                assert_eq!(import.module, "relocade");
                names.push(import.name.to_string());
            }
        }
    }
    names.sort();
    names
}

#[test]
fn physical_accesses_import_backing_map_and_host_transfers() {
    assert_eq!(
        imports(transfers()),
        [
            "guest",
            "mmioReads",
            "mmioWrites",
            "physicalMap",
            "writeCode"
        ]
    );
}

#[test]
fn one_slow_reader_serves_all_widths_and_callers_without_a_writer() {
    let mut program = Program::new();
    let memory = std::sync::Arc::new(Memory::Physical(PhysicalMemory::declare(&mut program)));
    for index in 0..12 {
        let memory = memory.clone();
        let function = program.function::<fn(u32) -> u64>(move |mut body, (address,)| {
            let access = memory.resolve_access(
                &mut body,
                &address,
                8,
                Intent::Read,
                None,
                Some(&mut exit::exception),
            )?;
            let value = match index % 4 {
                0 => memory
                    .read::<I8>(&mut body, &access, 0)?
                    .unsigned()
                    .extend::<I64>(),
                1 => memory
                    .read::<I16>(&mut body, &access, 0)?
                    .unsigned()
                    .extend::<I64>(),
                2 => memory
                    .read::<I32>(&mut body, &access, 0)?
                    .unsigned()
                    .extend::<I64>(),
                _ => memory.read::<I64>(&mut body, &access, 0)?,
            };
            body.return_(value)
        });
        program.export(&format!("read{index}"), function).unwrap();
    }
    let bytes = program.compile().unwrap();
    assert_eq!(imports(&bytes), ["guest", "mmioReads", "physicalMap"]);
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::FunctionSection(functions) = payload.unwrap() {
            assert_eq!(functions.count(), 13, "twelve callers share one reader");
        }
    }
}

#[test]
fn physical_fields_reuse_the_span_lookup() {
    let mut program = Program::new();
    let memory = Memory::Physical(PhysicalMemory::declare(&mut program));
    let function = program.function::<fn(u32) -> u64>(move |mut body, (address,)| {
        let access = memory.resolve_access(&mut body, &address, 8, Intent::Read, None, None)?;
        let low = access.field::<I32>(0)?;
        let high = access.field::<I32>(4)?;
        let low = memory.read::<I32>(&mut body, &low, 0)?;
        let high = memory.read::<I32>(&mut body, &high, 0)?;
        body.return_(
            low.unsigned()
                .extend::<I64>()
                .or(high.unsigned().extend::<I64>().shl(32)),
        )
    });
    program.export("read", function).unwrap();
    // One A20-mask, kind and payload lookup serves both fields.
    assert_eq!(physical_map_loads(&program.compile().unwrap(), "read"), 3);
}

#[test]
fn compiled_memory_operands_load_one_physical_page_record() {
    let compiler = crate::Compiler::from_config(REAL16);
    for code in [
        &[0x8b, 0x07][..],   // MOV AX,[BX]
        &[0x67, 0x89, 0x00], // MOV [EAX],AX
    ] {
        let module = compiler.compile_block(0x100, code, 1).unwrap();
        assert_eq!(
            physical_map_loads(&module.bytes, &module.entry),
            3,
            "one A20 mask, kind and payload for {code:02x?}"
        );
    }
}

fn physical_map_loads(bytes: &[u8], entry_name: &str) -> usize {
    use wasmparser::{ExternalKind, Operator, TypeRef};

    let mut memories = Vec::new();
    let mut function_index = 0;
    let mut entry = None;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.unwrap();
                    match import.ty {
                        TypeRef::Memory(_) => memories.push(import.name),
                        TypeRef::Func(_) => function_index += 1,
                        _ => {}
                    }
                }
            }
            Payload::ExportSection(exports) => {
                for export in exports {
                    let export = export.unwrap();
                    if export.name == entry_name && export.kind == ExternalKind::Func {
                        entry = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                if Some(function_index) == entry {
                    return body
                        .get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .map(Result::unwrap)
                        .filter(|op| {
                            matches!(op, Operator::I32Load { memarg } if memories[memarg.memory as usize] == "physicalMap")
                        })
                        .count();
                }
                function_index += 1;
            }
            _ => {}
        }
    }
    panic!("missing exported function {entry_name}");
}
