//! A callback in the first vector half can change routing of the second half.

use super::*;
use crate::test_step::MmioUpdate;
use wasm86_compiler::{Val, V128};

fn module(write: bool) -> TestModule {
    let mut program = Program::new();
    let memory = Memory::Physical(PhysicalMemory::declare(&mut program));
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64, Type::I64],
            },
            |mut body| {
                let access = memory.resolve_access(
                    &mut body,
                    &0x1ff8.into(),
                    16,
                    if write { Intent::Write } else { Intent::Read },
                    None,
                    Some(&mut exit::exception),
                )?;
                let value = if write {
                    let value = Val::<V128>::from(0x100f0e0d0c0b0a09_0807060504030201_u128);
                    memory.write(&mut body, &access, 0, &value)?;
                    value
                } else {
                    memory.read::<V128>(&mut body, &access, 0)?
                };
                body.return_((value.extract_lane::<I64>(0), value.extract_lane::<I64>(1)))
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
        execution_profile: None,
    })
}

fn rerouting(engine: Engine) {
    for write in [false, true] {
        let mut input = input();
        input.mmio_pages = vec![(1, 0x3000)];
        input.guest = vec![
            (0x3ff8, (1..=8).collect()),
            (0x7000, (9..=16).collect()),
            (0x5000, vec![0xee; 8]),
        ];
        if write {
            input.guest[0].1.fill(0xee);
            input.guest[1].1.fill(0xee);
        }
        input.mmio_updates = vec![MmioUpdate {
            map: vec![(2 * 8, vec![1, 0, 0, 0, 0, 0x70, 0, 0])],
            ..MmioUpdate::default()
        }];
        let observed = engine.observe(&module(write), &input, 1);
        assert_eq!(observed.events.len(), 2);
        assert_eq!(
            observed.events[0],
            if write {
                Event::MmioWrite {
                    address: 0x1ff8,
                    value: (1..=8).collect(),
                }
            } else {
                Event::MmioRead {
                    address: 0x1ff8,
                    bytes: 8,
                }
            }
        );
        let Event::Return { outcome, snapshot } = observed.events.last().unwrap() else {
            panic!("return expected")
        };
        assert_eq!(
            *outcome,
            Outcome::Returned(vec![
                Argument::I64(0x0807060504030201),
                Argument::I64(0x100f0e0d0c0b0a09)
            ])
        );
        let expected = if write {
            (0..16)
                .map(|i| {
                    (
                        if i < 8 { 0x3ff8 + i } else { 0x7000 + i - 8 },
                        (i + 1) as u8,
                    )
                })
                .collect()
        } else {
            vec![]
        };
        assert_eq!(snapshot.guest, Some(expected));
    }
}

#[test]
fn vector_halves_observe_mmio_rerouting() {
    rerouting(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_vector_halves_observe_mmio_rerouting() {
    rerouting(Engine::V8);
}
