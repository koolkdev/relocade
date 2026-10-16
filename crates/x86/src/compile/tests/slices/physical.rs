use super::*;

fn check_physical_rep_wrap(engine: Engine, frontend: Frontend) {
    // A full ROM-to-RAM chunk ends at SI wrap; the next chunk reads RAM at
    // offset zero. Every physical page has independent, nonidentity backing.
    let code = [0xf3, 0xa4];
    let mut cpu = initial();
    cpu.segments = crate::Segments::real_mode();
    cpu.registers.ecx = 0xcccc_012c;
    cpu.registers.esi = 0xaaaa_ff00;
    cpu.registers.edi = 0xbbbb_2f00;
    let mut input = Input::new(&cpu.to_bytes());
    input.budgets = vec![1, 1];
    input.observe_guest = true;
    input.observe_budget = true;
    input.physical_pages = vec![
        (1, 0x1000, true),
        (15, 0x5000, false),
        (0, 0x3000, true),
        (2, 0x4000, true),
        (3, 0x7000, true),
    ];
    input.guest = vec![
        (0x1000, code.to_vec()),
        (0x5f00, vec![1; 256]),
        (0x3000, vec![2; 44]),
    ];
    let block = TestModule::new(
        &Compiler::new(crate::ExecutionProfile::Real16)
            .with_execution_budget()
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    )
    .with_interpreter(real_interpreter());
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => real_interpreter(),
    };
    let observation = engine.observe(module, &input, 2);
    let [Event::Return {
        outcome: first_exit,
        snapshot: first,
    }, Event::Budget { remaining: -255 }, Event::Return {
        outcome: last_exit,
        snapshot: last,
    }, Event::Budget { remaining: -43 }] = observation.events.as_slice()
    else {
        panic!(
            "expected two bounded physical chunks: {:?}",
            observation.events
        );
    };
    let exhausted = Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)]);
    assert_eq!(*first_exit, exhausted);
    assert_eq!(*last_exit, exhausted);
    let state = CpuState::from_bytes(first.cpu.as_slice().try_into().unwrap());
    assert_eq!((state.eip, state.instruction_count), (0x1000, 0));
    assert_eq!(
        (
            state.registers.ecx,
            state.registers.esi,
            state.registers.edi
        ),
        (0xcccc_002c, 0xaaaa_0000, 0xbbbb_3000)
    );
    let copied: Vec<_> = (0..256).map(|offset| (0x4f00 + offset, 1)).collect();
    assert_eq!(first.guest, Some(copied.clone()));
    let state = CpuState::from_bytes(last.cpu.as_slice().try_into().unwrap());
    assert_eq!((state.eip, state.instruction_count), (0x1002, 1));
    assert_eq!(
        (
            state.registers.ecx,
            state.registers.esi,
            state.registers.edi
        ),
        (0xcccc_0000, 0xaaaa_002c, 0xbbbb_302c)
    );
    assert_eq!(
        last.guest,
        Some(
            copied
                .into_iter()
                .chain((0..44).map(|offset| (0x7000 + offset, 2)))
                .collect()
        )
    );
}
test_frontends!(physical_rep_wrap, check_physical_rep_wrap);
