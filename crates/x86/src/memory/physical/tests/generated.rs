use super::*;
use wasmparser::{Parser, Payload};

fn imports(bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for import in imports {
                let import = import.unwrap();
                assert_eq!(import.module, "wasm86");
                names.push(import.name.to_string());
            }
        }
    }
    names.sort();
    names
}

#[test]
fn physical_accesses_import_backing_map_and_only_mmio_callbacks() {
    assert_eq!(
        imports(transfers()),
        ["guest", "physicalMap", "readMmio", "writeMmio"]
    );
}

#[test]
fn one_slow_reader_serves_all_widths_and_callers_without_a_writer() {
    let mut program = Program::new();
    let memory = Memory::Physical(PhysicalMemory::declare(&mut program));
    for index in 0..12 {
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let address = body.parameter::<I32>(0)?;
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
                },
            )
            .unwrap();
        program.export(&format!("read{index}"), function).unwrap();
    }
    let bytes = program.compile().unwrap();
    assert_eq!(imports(&bytes), ["guest", "physicalMap", "readMmio"]);
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::FunctionSection(functions) = payload.unwrap() {
            assert_eq!(functions.count(), 13, "twelve callers share one reader");
        }
    }
}
