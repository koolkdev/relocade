use super::*;

fn check_rep_completion(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa4];
    let block = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    )
    .with_interpreter(step());
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => step(),
    };
    for count in [0, 300u32] {
        let mut cpu = initial();
        cpu.registers.ecx = count;
        cpu.registers.esi = 0x2000;
        cpu.registers.edi = 0x3000;
        let contents: Vec<_> = (0..count).map(|i| (i % 251 + 1) as u8).collect();
        let mut input = input(&code, cpu, &[1]);
        input.guest.push((0x2000, contents.clone()));
        input.observe_budget = true;
        let observation = engine.observe(module, &input, 1);
        let [.., Event::Return { snapshot, .. }, Event::Budget { remaining }] =
            observation.events.as_slice()
        else {
            panic!("expected completed REP and its remaining allowance");
        };
        assert_eq!(*remaining, 0);
        let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
        assert_eq!((state.eip, state.instruction_count), (0x1002, 1));
        assert_eq!(
            (
                state.registers.ecx,
                state.registers.esi,
                state.registers.edi
            ),
            (0, 0x2000 + count, 0x3000 + count)
        );
        assert_eq!(
            snapshot.guest,
            Some(
                contents
                    .into_iter()
                    .enumerate()
                    .map(|(i, byte)| (0x3000 + i as u32, byte))
                    .collect()
            )
        );
    }
}
test_frontends!(rep_completes_as_one_instruction, check_rep_completion);

fn check_faulted_rep(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa4];
    let mut cpu = initial();
    cpu.registers.ecx = 5;
    cpu.registers.esi = 0x2ffd;
    cpu.registers.edi = 0x3ffd;
    let mut input = input(&code, cpu, &[8]);
    input.guest.push((0x2ffd, vec![7, 8, 9]));
    input.observe_budget = true;
    let block = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    )
    .with_interpreter(interpreter());
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => interpreter(),
    };
    let observation = engine.observe(module, &input, 1);
    let [.., Event::Return { outcome, snapshot }, Event::Budget { remaining }] =
        observation.events.as_slice()
    else {
        panic!("expected the fault and its remaining allowance");
    };
    assert_eq!(*remaining, 8);
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(0x0004_0002_0000_4000)])
    );
    let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!((state.eip, state.instruction_count), (0x1000, 0));
    assert_eq!(
        (
            state.registers.ecx,
            state.registers.esi,
            state.registers.edi
        ),
        (2, 0x3000, 0x4000)
    );
    assert_eq!(
        snapshot.guest,
        Some(vec![(0x3ffd, 7), (0x3ffe, 8), (0x3fff, 9)])
    );
}
test_frontends!(
    faulted_rep_preserves_progress_without_retiring,
    check_faulted_rep
);
