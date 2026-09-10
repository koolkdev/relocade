use super::*;

fn define_unused_read<T: MemoryInt>(program: &mut Program, memory: &Memory, name: &str) {
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                result: Some(Type::I64),
            },
            |mut body| {
                let address = body.parameter::<I32>(0)?;
                let access =
                    memory.resolve_access::<T>(&mut body, &address, Intent::Read, return_fault)?;
                let _ = memory.read(&mut body, &access)?;
                body.return_(7)
            },
        )
        .unwrap();
    program.export(name, function).unwrap();
}

#[test]
fn unused_values_do_not_remove_required_reads_at_any_storage_width() {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    define_unused_read::<I8>(&mut program, &memory, "read8");
    define_unused_read::<I16>(&mut program, &memory, "read16");
    define_unused_read::<I32>(&mut program, &memory, "read32");
    define_unused_read::<I64>(&mut program, &memory, "read64");
    let bytes = program.compile().unwrap();

    for entry in ["read8", "read16", "read32", "read64"] {
        let module = TestModule::new(&crate::CompiledModule {
            bytes: bytes.clone(),
            entry: entry.into(),
        });
        for (frame, outcome) in [
            (0x8000_u32, Outcome::Returned(Some(Argument::I64(7)))),
            (0x10000, Outcome::Trap),
        ] {
            let cpu = CpuState::filled(0xa5).to_bytes().to_vec();
            let input = Input {
                guest: vec![(0x8000, vec![0x5a; 8])],
                // Both entries are present and read-only; only the physical
                // load can detect that the second frame is outside guest RAM.
                machine: vec![(16, (frame | 1).to_le_bytes().to_vec())],
                arguments: vec![Argument::I32(0x4000)],
                observe_guest: true,
                ..Input::new(&cpu)
            };
            assert_eq!(
                module.observe(&input, 1),
                Observation {
                    events: vec![Event::Return {
                        outcome,
                        snapshot: Snapshot {
                            cpu,
                            guest: Some(vec![])
                        },
                    }],
                    guest_unchanged: true,
                    machine_unchanged: true,
                },
                "{entry}, frame {frame:#x}",
            );
        }
    }
}
