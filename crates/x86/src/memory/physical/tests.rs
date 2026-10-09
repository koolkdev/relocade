use std::sync::OnceLock;

use super::PhysicalMemory;

use wasm86_compiler::{MemoryInt, Program, Signature, Type, I16, I32, I64, I8};
mod generated;
mod routing;
mod vectors;

use crate::{
    alu::OperandUpdate,
    memory::{Intent, Memory},
    state::exit,
    test_step::{Argument, Engine, Event, Input, Outcome, TestModule},
    CompiledModule,
};

fn define_transfers<T: MemoryInt + crate::memory::TransferType>(
    program: &mut Program,
    memory: &Memory,
) {
    for write in [false, true] {
        let function = program
            .function(
                Signature {
                    parameters: if write {
                        vec![Type::I32, T::TYPE]
                    } else {
                        vec![Type::I32]
                    },
                    results: vec![Type::I64],
                },
                |mut body| {
                    let address = body.parameter::<I32>(0)?;
                    let access = memory.resolve_access(
                        &mut body,
                        &address,
                        T::BYTES,
                        if write { Intent::Write } else { Intent::Read },
                        None,
                        Some(&mut exit::exception),
                    )?;
                    if write {
                        let value = body.parameter::<T>(1)?;
                        memory.write(&mut body, &access, 0, &value)?;
                        body.return_(7)
                    } else {
                        let value = memory.read::<T>(&mut body, &access, 0)?;
                        body.return_(value.unsigned().extend::<I64>())
                    }
                },
            )
            .unwrap();
        program
            .export(
                &format!("{}{}", if write { "write" } else { "read" }, T::BYTES * 8),
                function,
            )
            .unwrap();
    }
}

fn transfers() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let mut program = Program::new();
        let memory = Memory::Physical(PhysicalMemory::declare(&mut program));
        define_transfers::<I8>(&mut program, &memory);
        define_transfers::<I16>(&mut program, &memory);
        define_transfers::<I32>(&mut program, &memory);
        define_transfers::<I64>(&mut program, &memory);
        program.compile().unwrap()
    })
}

fn input() -> Input {
    Input {
        guest: vec![
            (0x3fff, vec![0x88]),
            (0x5000, vec![0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]),
        ],
        physical_pages: vec![
            (1, 0x3000, true),
            (2, 0x5000, true),
            (9, 0x3000, false),
            (10, 0x5000, false),
        ],
        observe_mmio: true,
        observe_guest: true,
        ..Input::new(&[])
    }
}

fn widths(engine: Engine, mmio: bool) {
    for (bits, expected_read, written) in [
        (8, 0x88, &[0x91][..]),
        (16, 0x7788, &[0x91, 0xa2][..]),
        (32, 0x5566_7788, &[0x91, 0xa2, 0xb3, 0xc4][..]),
        (
            64,
            0x1122_3344_5566_7788,
            &[0x91, 0xa2, 0xb3, 0xc4, 0xd5, 0xe6, 0xf7, 0x88][..],
        ),
    ] {
        for write in [false, true] {
            let module = TestModule::new(&CompiledModule {
                bytes: transfers().to_vec(),
                entry: format!("{}{bits}", if write { "write" } else { "read" }),
                execution_profile: None,
            });
            let mut input = input();
            if mmio {
                input.mmio_pages = vec![(1, 0x3000), (2, 0x5000)];
            }
            input.arguments.push(Argument::I32(0x1fff));
            if write {
                let mut value = [0; 8];
                value[..written.len()].copy_from_slice(written);
                input.arguments.push(if bits == 64 {
                    Argument::I64(i64::from_le_bytes(value))
                } else {
                    Argument::I32(i64::from_le_bytes(value) as i32)
                });
            }
            let observed = engine.observe(&module, &input, 1);
            assert_eq!(observed.events.len(), if mmio { 2 } else { 1 });
            if mmio {
                assert_eq!(
                    observed.events[0],
                    if write {
                        Event::MmioWrite {
                            address: 0x1fff,
                            value: written.to_vec(),
                        }
                    } else {
                        Event::MmioRead {
                            address: 0x1fff,
                            bytes: bits / 8,
                        }
                    }
                );
            }
            let Event::Return { outcome, snapshot } = observed.events.last().unwrap() else {
                panic!("return")
            };
            assert_eq!(
                outcome,
                &Outcome::Returned(vec![Argument::I64(if write { 7 } else { expected_read })])
            );
            let changes: Vec<_> = if write {
                written
                    .iter()
                    .enumerate()
                    .map(|(index, &byte)| {
                        (
                            if index == 0 {
                                0x3fff
                            } else {
                                0x4fff + index as u32
                            },
                            byte,
                        )
                    })
                    .collect()
            } else {
                vec![]
            };
            assert_eq!(snapshot.guest, Some(changes));
            assert_eq!(observed.guest_unchanged, !write);
            assert!(observed.machine_unchanged);
        }
    }
}

fn updates(engine: Engine) {
    for matches in [false, true] {
        let mut program = Program::new();
        let memory = Memory::Physical(PhysicalMemory::declare(&mut program));
        let function = program
            .function(
                Signature {
                    parameters: vec![],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let unused = memory.resolve_access(
                        &mut body,
                        &0x8000.into(),
                        1,
                        Intent::Read,
                        None,
                        Some(&mut exit::exception),
                    )?;
                    memory.read::<I8>(&mut body, &unused, 0)?;
                    let access = memory.resolve_access(
                        &mut body,
                        &0x1fff.into(),
                        8,
                        Intent::Write,
                        None,
                        Some(&mut exit::exception),
                    )?;
                    let before = memory.atomic_update(
                        &mut body,
                        &access,
                        &OperandUpdate::<I64>::CompareExchange {
                            expected: if matches {
                                0x1122_3344_5566_7788u64.into()
                            } else {
                                0.into()
                            },
                            replacement: 0x8877_6655_4433_2211u64.into(),
                        },
                    )?;
                    let alias = memory.resolve_access(
                        &mut body,
                        &0x9fff.into(),
                        8,
                        Intent::Read,
                        None,
                        Some(&mut exit::exception),
                    )?;
                    let after = memory.read::<I64>(&mut body, &alias, 0)?;
                    body.return_(
                        before
                            .eq(0x1122_3344_5566_7788u64)
                            .and(after.eq(if matches {
                                0x8877_6655_4433_2211u64
                            } else {
                                0x1122_3344_5566_7788u64
                            }))
                            .unsigned()
                            .extend::<I64>(),
                    )
                },
            )
            .unwrap();
        program.export("update", function).unwrap();
        let module = TestModule::new(&CompiledModule {
            bytes: program.compile().unwrap(),
            entry: "update".into(),
            execution_profile: None,
        });
        let mut input = input();
        input.mmio_pages = vec![(1, 0x3000), (2, 0x5000), (8, 0x6000)];
        let observed = engine.observe(&module, &input, 1);
        assert_eq!(
            &observed.events[..3],
            &[
                Event::MmioRead {
                    address: 0x8000,
                    bytes: 1
                },
                Event::MmioRead {
                    address: 0x1fff,
                    bytes: 8
                },
                Event::MmioWrite {
                    address: 0x1fff,
                    value: if matches {
                        vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
                    } else {
                        vec![0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
                    }
                },
            ]
        );
        assert_eq!(observed.events.len(), 4);
        let Event::Return { outcome, .. } = &observed.events[3] else {
            panic!("return")
        };
        assert_eq!(outcome, &Outcome::Returned(vec![Argument::I64(1)]));
        assert_eq!(observed.guest_unchanged, !matches);
        assert!(observed.machine_unchanged);
    }
}

#[test]
fn widths_crossings_and_callback_effects() {
    widths(Engine::Wasmtime, false);
    widths(Engine::Wasmtime, true);
    updates(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_widths_crossings_and_callback_effects() {
    widths(Engine::V8, false);
    widths(Engine::V8, true);
    updates(Engine::V8);
}
