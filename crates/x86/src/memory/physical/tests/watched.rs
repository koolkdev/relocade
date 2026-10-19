use super::*;

fn watched_transfers() {
    let mut input = input();
    input.a20_enabled = false;
    input.watched_backing_pages = vec![3];
    input.guest.push((0x3000, vec![7]));
    for write in [false, true] {
        let module = TestModule::new(&CompiledModule {
            bytes: transfers().to_vec(),
            entry: if write { "write8" } else { "read8" }.into(),
            execution_target: None,
        });
        input.arguments = vec![Argument::I32(0x1000)];
        if write {
            input.arguments.push(Argument::I32(10));
        }
        let observed = module.observe(&input, 1);
        assert_eq!(observed.events.len(), if write { 2 } else { 1 });
        if write {
            assert_eq!(
                observed.events[0],
                Event::CodeWrite {
                    backing: 0x3000,
                    value: vec![10]
                }
            );
        }
        let Event::Return { outcome, snapshot } = observed.events.last().unwrap() else {
            panic!("return")
        };
        assert_eq!(outcome, &Outcome::Returned(vec![Argument::I64(7)]));
        assert_eq!(
            snapshot.guest,
            Some(if write { vec![(0x3000, 10)] } else { vec![] })
        );
    }
    input
        .physical_pages
        .extend([(17, 0x3000, true), (18, 0x5000, true)]);
    for address in [0x1fff, 0x9fff, 0x101fff] {
        let module = TestModule::new(&CompiledModule {
            bytes: transfers().to_vec(),
            entry: "read64".into(),
            execution_target: None,
        });
        input.arguments = vec![Argument::I32(address)];
        let observed = module.observe(&input, 1);
        assert_eq!(observed.events.len(), 1);
        let Event::Return { outcome, .. } = &observed.events[0] else {
            panic!("return")
        };
        assert_eq!(
            outcome,
            &Outcome::Returned(vec![Argument::I64(0x1122_3344_5566_7788)])
        );
        assert!(observed.guest_unchanged);
    }
    let module = TestModule::new(&CompiledModule {
        bytes: transfers().to_vec(),
        entry: "write64".into(),
        execution_target: None,
    });
    input.arguments = vec![Argument::I32(0x11fff), Argument::I64(0x1234_5678_9abc_def0)];
    let observed = module.observe(&input, 1);
    assert_eq!(observed.events.len(), 2);
    assert_eq!(
        observed.events[0],
        Event::CodeWrite {
            backing: 0x3fff,
            value: vec![0xf0]
        }
    );
    let Event::Return { outcome, snapshot } = &observed.events[1] else {
        panic!("return")
    };
    assert_eq!(outcome, &Outcome::Returned(vec![Argument::I64(7)]));
    assert_eq!(
        snapshot.guest,
        Some(vec![
            (0x3fff, 0xf0),
            (0x5000, 0xde),
            (0x5001, 0xbc),
            (0x5002, 0x9a),
            (0x5003, 0x78),
            (0x5004, 0x56),
            (0x5005, 0x34),
            (0x5006, 0x12)
        ])
    );
}

#[test]
fn watched_ram_reads_and_writes_through_aliases() {
    watched_transfers();
}
