use super::*;
use crate::test_step::{MmioUpdate, Observation};

fn transfer(engine: Engine, input: &Input, entry: &str) -> Observation {
    let module = TestModule::new(&CompiledModule {
        bytes: transfers().to_vec(),
        entry: entry.into(),
        execution_profile: None,
    });
    engine.observe(&module, input, 1)
}

fn value(observation: &Observation) -> i64 {
    let Some(Event::Return {
        outcome: Outcome::Returned(results),
        ..
    }) = observation.events.last()
    else {
        panic!("successful return expected");
    };
    let [Argument::I64(value)] = results[..] else {
        panic!("one i64 result expected")
    };
    value
}

fn mixed(engine: Engine) {
    for mmio_first in [false, true] {
        for first_bytes in [1, 3, 5, 7] {
            let address = 0x2000 - first_bytes;
            let mut input = input();
            input.guest = vec![
                (0x4000 - first_bytes, (1..=first_bytes as u8).collect()),
                (0x5000, (first_bytes as u8 + 1..=8).collect()),
            ];
            input.mmio_pages = vec![if mmio_first { (1, 0x3000) } else { (2, 0x5000) }];
            input.arguments = vec![Argument::I32(address as i32)];
            let read = transfer(engine, &input, "read64");
            assert_eq!(value(&read), 0x0807_0605_0403_0201);
            assert_eq!(read.events.len(), 2);
            assert_eq!(
                read.events[0],
                Event::MmioRead {
                    address: if mmio_first { address } else { 0x2000 },
                    bytes: if mmio_first {
                        first_bytes
                    } else {
                        8 - first_bytes
                    },
                }
            );
            let bytes = [0x91, 0xa2, 0xb3, 0xc4, 0xd5, 0xe6, 0xf7, 0x88];
            input
                .arguments
                .push(Argument::I64(i64::from_le_bytes(bytes)));
            let write = transfer(engine, &input, "write64");
            assert_eq!(write.events.len(), 2);
            assert_eq!(
                write.events[0],
                Event::MmioWrite {
                    address: if mmio_first { address } else { 0x2000 },
                    value: if mmio_first {
                        &bytes[..first_bytes as usize]
                    } else {
                        &bytes[first_bytes as usize..]
                    }
                    .to_vec(),
                }
            );
            let Event::Return { snapshot, .. } = write.events.last().unwrap() else {
                unreachable!()
            };
            let expected = bytes
                .iter()
                .enumerate()
                .map(|(index, &byte)| {
                    (
                        if index < first_bytes as usize {
                            0x4000 - first_bytes + index as u32
                        } else {
                            0x5000 + index as u32 - first_bytes
                        },
                        byte,
                    )
                })
                .collect();
            assert_eq!(snapshot.guest, Some(expected));
        }
    }
}

fn backing_and_holes(engine: Engine) {
    for address in [0x1801, 0x9fff, 0xafff, 0xb000] {
        let mut input = input();
        input
            .guest
            .push((0x3801, vec![0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]));
        input.guest.push((0x5fff, vec![0x42]));
        input.arguments = vec![Argument::I32(address)];
        let read = transfer(engine, &input, "read64");
        assert_eq!(
            read.events.len(),
            1,
            "ordinary backing and holes never call the host"
        );
        assert_eq!(
            value(&read),
            match address {
                0x1801 | 0x9fff => 0x1122_3344_5566_7788,
                0xafff => 0xffff_ffff_ffff_ff42u64 as i64,
                _ => -1,
            }
        );
        input
            .arguments
            .push(Argument::I64(0x8877_6655_4433_2211u64 as i64));
        let write = transfer(engine, &input, "write64");
        assert_eq!(write.events.len(), 1);
        assert_eq!(write.guest_unchanged, address != 0x1801);
    }
    // The low hole's FF bits must not overwrite a later MMIO result.
    let mut input = input();
    input.physical_pages.clear();
    input.mmio_pages = vec![(2, 0x5000)];
    input.arguments = vec![Argument::I32(0x1fff)];
    let read = transfer(engine, &input, "read32");
    assert_eq!(value(&read), 0x5566_77ff);
    assert_eq!(
        read.events[0],
        Event::MmioRead {
            address: 0x2000,
            bytes: 3
        }
    );
}

fn remapping(engine: Engine) {
    for write in [false, true] {
        let mut input = input();
        input.mmio_pages = vec![(1, 0x3000)];
        input.guest.push((
            0x7000,
            if write {
                vec![0xee; 3]
            } else {
                vec![0x22, 0x33, 0x44]
            },
        ));
        input.arguments = vec![Argument::I32(0x1fff)];
        // Replace page 2's backing after the first (MMIO) byte transfers.
        input.mmio_updates = vec![MmioUpdate {
            map: vec![(2 * 8, vec![1, 0, 0, 0, 0, 0x70, 0, 0])],
            ..MmioUpdate::default()
        }];
        if write {
            input.arguments.push(Argument::I32(0x4433_2211));
        }
        let observed = transfer(engine, &input, if write { "write32" } else { "read32" });
        assert_eq!(observed.events.len(), 2);
        assert_eq!(value(&observed), if write { 7 } else { 0x4433_2288 });
        let Event::Return { snapshot, .. } = observed.events.last().unwrap() else {
            unreachable!()
        };
        assert_eq!(
            snapshot.guest,
            Some(if write {
                vec![
                    (0x3fff, 0x11),
                    (0x7000, 0x22),
                    (0x7001, 0x33),
                    (0x7002, 0x44),
                ]
            } else {
                vec![]
            })
        );
    }
    // The next direct read must see RAM changes made by an MMIO read callback.
    let mut program = Program::new();
    let memory = Memory::Physical(PhysicalMemory::declare(&mut program));
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                let ram = memory.resolve_access(
                    &mut body,
                    &0x2000.into(),
                    1,
                    Intent::Read,
                    None,
                    Some(&mut exit::exception),
                )?;
                let old = memory.read::<I8>(&mut body, &ram, 0)?;
                let device = memory.resolve_access(
                    &mut body,
                    &0x8000.into(),
                    1,
                    Intent::Read,
                    None,
                    Some(&mut exit::exception),
                )?;
                memory.read::<I8>(&mut body, &device, 0)?;
                let new = memory.read::<I8>(&mut body, &ram, 0)?;
                body.return_(
                    old.unsigned()
                        .extend::<I64>()
                        .shl(8)
                        .or(new.unsigned().extend::<I64>()),
                )
            },
        )
        .unwrap();
    program.export("read", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "read".into(),
        execution_profile: None,
    });
    let mut input = input();
    input.mmio_pages = vec![(8, 0x6000)];
    input.mmio_updates = vec![MmioUpdate {
        guest: vec![(0x5000, vec![0x42])],
        ..MmioUpdate::default()
    }];
    let observed = engine.observe(&module, &input, 1);
    assert_eq!(value(&observed), 0x7742);
    assert_eq!(observed.events.len(), 2);
}

fn last_real_mode_page(engine: Engine) {
    let mut input = input();
    input.physical_pages = vec![(0x10f, 0x3000, true)];
    input.guest = vec![(0x3fe8, vec![0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11])];
    input.arguments = vec![Argument::I32(0x10ffe8)];
    assert_eq!(
        value(&transfer(engine, &input, "read64")),
        0x1122_3344_5566_7788
    );
    input.mmio_pages = vec![(0x10f, 0x3000)];
    let observed = transfer(engine, &input, "read64");
    assert_eq!(value(&observed), 0x1122_3344_5566_7788);
    assert_eq!(observed.events.len(), 2);
    assert_eq!(
        observed.events[0],
        Event::MmioRead {
            address: 0x10ffe8,
            bytes: 8
        }
    );
}

fn a20(engine: Engine) {
    for enabled in [false, true] {
        for mmio in [false, true] {
            let mut input = input();
            input.a20_enabled = enabled;
            input.physical_pages = vec![
                (0xff, 0x3000, true),
                (0, 0x5000, true),
                (0x100, 0x7000, true),
            ];
            input.guest.push((0x7000, vec![0x11, 0x22, 0x33]));
            if mmio {
                input.mmio_pages = vec![(0xff, 0x3000), (0, 0x5000), (0x100, 0x7000)];
            }
            input.arguments = vec![Argument::I32(0xfffff)];
            let read = transfer(engine, &input, "read32");
            assert_eq!(
                value(&read),
                if enabled { 0x3322_1188 } else { 0x5566_7788 }
            );
            if mmio {
                let expected = if enabled {
                    vec![Event::MmioRead {
                        address: 0xfffff,
                        bytes: 4,
                    }]
                } else {
                    vec![
                        Event::MmioRead {
                            address: 0xfffff,
                            bytes: 1,
                        },
                        Event::MmioRead {
                            address: 0,
                            bytes: 3,
                        },
                    ]
                };
                assert_eq!(&read.events[..read.events.len() - 1], expected);
            }
            input.arguments.push(Argument::I32(0x4433_2211));
            let write = transfer(engine, &input, "write32");
            let Event::Return { snapshot, .. } = write.events.last().unwrap() else {
                unreachable!()
            };
            let base = if enabled { 0x7000 } else { 0x5000 };
            assert_eq!(
                snapshot.guest,
                Some(vec![
                    (0x3fff, 0x11),
                    (base, 0x22),
                    (base + 1, 0x33),
                    (base + 2, 0x44)
                ])
            );

            // A callback changes the gate after the high byte was issued. The
            // remaining bytes must resolve from their original linear address.
            input.mmio_pages = vec![(0xff, 0x3000)];
            input.mmio_updates = vec![MmioUpdate {
                map: vec![(
                    2176,
                    if enabled {
                        vec![0xff, 0xff, 0xef, 0xff]
                    } else {
                        vec![0xff; 4]
                    },
                )],
                ..MmioUpdate::default()
            }];
            input.arguments.truncate(1);
            let read = transfer(engine, &input, "read32");
            assert_eq!(
                value(&read),
                if enabled { 0x5566_7788 } else { 0x3322_1188 }
            );
        }
    }
    // A direct high address uses the low page's kind, including ROM policy.
    let mut input = input();
    input.a20_enabled = false;
    input.physical_pages = vec![(0, 0x5000, false)];
    input.arguments = vec![Argument::I32(0x100000)];
    assert_eq!(value(&transfer(engine, &input, "read32")), 0x4455_6677);
    input.arguments.push(Argument::I32(0));
    assert!(transfer(engine, &input, "write32").guest_unchanged);
}

#[test]
fn routing_preserves_mmio_portions_and_live_mappings() {
    mixed(Engine::Wasmtime);
    backing_and_holes(Engine::Wasmtime);
    remapping(Engine::Wasmtime);
    last_real_mode_page(Engine::Wasmtime);
    a20(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_routing_preserves_mmio_portions_and_live_mappings() {
    mixed(Engine::V8);
    backing_and_holes(Engine::V8);
    remapping(Engine::V8);
    last_real_mode_page(Engine::V8);
    a20(Engine::V8);
}
