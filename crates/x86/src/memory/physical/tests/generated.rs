use super::*;
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
fn physical_accesses_import_backing_map_and_mmio_tables() {
    assert_eq!(
        imports(transfers()),
        ["guest", "mmioReads", "mmioWrites", "physicalMap"]
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
    use wasmparser::{ExternalKind, Operator, TypeRef};

    let mut program = Program::new();
    let memory = Memory::Physical(PhysicalMemory::declare(&mut program));
    let function = program.function::<fn(u32) -> u64>(move |mut body, (address,)| {
        let access = memory.resolve_access(&mut body, &address, 8, Intent::Read, None, None)?;
        let low = memory.read::<I32>(&mut body, &access, 0)?;
        let high = memory.read::<I32>(&mut body, &access, 4)?;
        body.return_(
            low.unsigned()
                .extend::<I64>()
                .or(high.unsigned().extend::<I64>().shl(32)),
        )
    });
    program.export("read", function).unwrap();
    let bytes = program.compile().unwrap();
    let mut memories = Vec::new();
    let mut function_index = 0;
    let mut entry = None;
    let mut map_loads = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
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
                    if export.name == "read" && export.kind == ExternalKind::Func {
                        entry = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                if Some(function_index) == entry {
                    for operator in body.get_operators_reader().unwrap() {
                        if let Operator::I32Load { memarg } = operator.unwrap() {
                            if memories[memarg.memory as usize] == "physicalMap" {
                                map_loads += 1;
                            }
                        }
                    }
                }
                function_index += 1;
            }
            _ => {}
        }
    }
    // One A20-mask, kind and backing lookup serves both fields, even without a JIT guard.
    assert_eq!(map_loads, 3);
}
