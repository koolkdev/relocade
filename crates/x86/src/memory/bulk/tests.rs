use crate::{
    memory::{Intent, Memory},
    test_step::{Argument, Event, Input, Outcome, TestModule},
    CompiledModule, ExecutionProfile,
};
use relocade_compiler::{Program, I16};

fn transfers(fill: bool) -> TestModule {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program, ExecutionProfile::Real16).unwrap();
    let function = program
        .function::<fn(u32, u32, u32) -> u64>(|mut body, (destination, source, bytes)| {
            let destination = memory.check_direct_access(
                &mut body,
                &destination,
                &bytes,
                Intent::Write,
                None,
                None,
            )?;
            body.if_(&destination.unavailable, |denied| denied.return_(0u64))?;
            if fill {
                memory.fill::<I16>(&mut body, &destination.physical, &0x2211.into(), &bytes)?;
            } else {
                let source = memory.check_direct_access(
                    &mut body,
                    &source,
                    &bytes,
                    Intent::Read,
                    None,
                    None,
                )?;
                body.if_(&source.unavailable, |denied| denied.return_(0u64))?;
                memory.copy(&mut body, &destination.physical, &source.physical, &bytes)?;
            }
            body.return_(1u64)
        })
        .unwrap();
    program.export("transfer", function).unwrap();
    TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "transfer".into(),
        execution_target: None,
    })
}

#[test]
fn physical_bulk_transfers_require_proven_backing() {
    for fill in [false, true] {
        let module = transfers(fill);
        for (destination, source, bytes, allowed) in [
            (0x1000, 0x2000, 4, true),
            (0x1000, 0x2000, 8, true),
            (0x1ffc, 0x2000, 4, true),
            (0x1000, 0x2000, 0, false),
            (0x1000, 0x2000, 4097, false),
            (0x1ffe, 0x2000, 4, false),
            (0x2000, 0x2000, 4, false), // ROM cannot be a bulk destination.
            (0x8000, 0x2000, 4, false), // MMIO needs checked transfers.
            (0x9000, 0x2000, 4, false), // Holes have no direct backing.
        ] {
            let mut input = Input::new(&[]);
            input.physical_pages = vec![(1, 0x3000, true), (2, 0x5000, false)];
            input.mmio_pages = vec![(8, 0x6000)];
            input.guest = vec![(0x5000, (1..=8).collect())];
            input.observe_guest = true;
            input.observe_mmio = true;
            input.arguments = vec![
                Argument::I32(destination),
                Argument::I32(source),
                Argument::I32(bytes),
            ];
            let observed = module.observe(&input, 1);
            let [Event::Return { outcome, snapshot }] = observed.events.as_slice() else {
                panic!("a direct-range proof must not invoke a device");
            };
            assert_eq!(
                *outcome,
                Outcome::Returned(vec![Argument::I64(i64::from(allowed))])
            );
            let expected = if allowed {
                (0..bytes as u32)
                    .map(|i| {
                        (
                            0x3000 + (destination as u32 & 4095) + i,
                            if fill {
                                [0x11, 0x22][i as usize % 2]
                            } else {
                                (i + 1) as u8
                            },
                        )
                    })
                    .collect()
            } else {
                vec![]
            };
            assert_eq!(snapshot.guest, Some(expected));
        }
    }
}
