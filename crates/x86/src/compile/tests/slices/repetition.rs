use super::*;

fn check_fault_work(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa4]; // Three bytes fit before the destination page fault.
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
    let (module, calls) = match frontend {
        Frontend::Block => (&block, 2),
        Frontend::Interpreter => (interpreter(), 1),
    };
    let observation = engine.observe(module, &input, calls);
    let [.., Event::Return { outcome, snapshot }, Event::Budget { remaining }] =
        observation.events.as_slice()
    else {
        panic!("expected the fault and its remaining work");
    };
    assert_eq!(*remaining, 5);
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(0x0004_0002_0000_4000)])
    );
    let cpu = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!((cpu.eip, cpu.instruction_count), (0x1000, 0));
    assert_eq!(
        (cpu.registers.ecx, cpu.registers.esi, cpu.registers.edi),
        (2, 0x3000, 0x4000)
    );
    assert_eq!(
        snapshot.guest,
        Some(vec![(0x3ffd, 7), (0x3ffe, 8), (0x3fff, 9)])
    );
}
test_frontends!(fault_work, check_fault_work);

fn check_rep_load(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xac, 0xeb, 0]; // REP LODSB; JMP +0
    let mut cpu = initial();
    cpu.registers.ecx = 3;
    cpu.registers.esi = 0x2000;
    cpu.registers.eax = 0x12345600;
    let mut input = input(&code, cpu, &[1, 1, 1]);
    input.guest.push((0x2000, vec![7, 8, 9]));
    let observations = run(engine, frontend, &code, 1, &input);
    for (index, (exit, state)) in observations.iter().enumerate() {
        assert_eq!(state.registers.eax, 0x12345607 + index as u32);
        assert_eq!(state.registers.ecx, 2 - index as u32);
        assert_eq!(state.registers.esi, 0x2001 + index as u32);
        assert_eq!(state.eip, if index == 2 { 0x1002 } else { 0x1000 });
        assert_eq!(state.instruction_count, u32::from(index == 2));
        if index < 2 {
            assert_eq!(*exit, SLICE_EXHAUSTED);
        }
    }
}
test_frontends!(rep_load, check_rep_load);

fn check_backward_narrow_rep(engine: Engine, frontend: Frontend) {
    let code = [0x67, 0xf3, 0xac]; // Address-size override; REP LODSB
    let mut cpu = initial();
    cpu.registers.ecx = 0xaaaa_0002;
    cpu.registers.esi = 0xbbbb_2001;
    cpu.registers.eax = 0x12345600;
    cpu.flags.bytes.df = 1;
    let mut input = input(&code, cpu, &[1, 1]);
    input.guest.push((0x2000, vec![7, 8]));
    let observations = run(engine, frontend, &code, 1, &input);
    assert_eq!(observations[0].0, SLICE_EXHAUSTED);
    for (index, (_, state)) in observations.iter().enumerate() {
        assert_eq!(state.registers.eax, 0x12345608 - index as u32);
        assert_eq!(state.registers.ecx, 0xaaaa_0001 - index as u32);
        assert_eq!(state.registers.esi, 0xbbbb_2000 - index as u32);
        assert_eq!(state.flags, cpu.flags);
        assert_eq!(state.instruction_count, index as u32);
        assert_eq!(state.eip, if index == 0 { 0x1000 } else { 0x1003 });
    }
}
test_frontends!(backward_narrow_rep, check_backward_narrow_rep);

fn check_rep_copy(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa4];
    let mut cpu = initial();
    cpu.registers.ecx = u32::MAX;
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x3000;
    let mut input = input(&code, cpu, &[2]);
    input.guest.push((0x2000, vec![7, 8, 9]));
    let block = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    );
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => interpreter(),
    };
    let observation = engine.observe(module, &input, 1);
    let [Event::Return { outcome, snapshot }] = observation.events.as_slice() else {
        panic!("yield without dispatch")
    };
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)])
    );
    let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!(state.eip, 0x1000);
    assert_eq!(state.instruction_count, 0);
    assert_eq!(state.registers.ecx, u32::MAX - 2);
    assert_eq!(state.registers.esi, 0x2002);
    assert_eq!(state.registers.edi, 0x3002);
    assert_eq!(snapshot.guest, Some(vec![(0x3000, 7), (0x3001, 8)]));
}
test_frontends!(rep_copy, check_rep_copy);

fn check_rep_chunk_fallback(engine: Engine, frontend: Frontend) {
    // The first dword straddles nonconsecutive backing pages. One checked
    // element crosses the boundary; the remaining three fit a direct chunk.
    let code = [0xf3, 0xa5];
    let mut cpu = initial();
    cpu.registers.ecx = 4;
    cpu.registers.esi = 0x2ffe;
    cpu.registers.edi = 0x4000;
    let mut input = input(&code, cpu, &[10, 10]);
    input.machine.push((12, 0x6003u32.to_le_bytes().to_vec()));
    input.machine.push((16, 0x4003u32.to_le_bytes().to_vec()));
    input.guest.push((0x2ffe, vec![1, 2]));
    input.guest.push((0x6000, (3..=16).collect()));
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
    let observation = engine.observe(module, &input, 2);
    let returned: Vec<_> = observation
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Return { snapshot, .. } => Some(snapshot),
            _ => None,
        })
        .collect();
    let first = CpuState::from_bytes(returned[0].cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (first.eip, first.instruction_count, first.registers.ecx),
        (0x1000, 0, 3)
    );
    assert_eq!((first.registers.esi, first.registers.edi), (0x3002, 0x4004));
    assert_eq!(
        returned[0].guest,
        Some((0..4).map(|i| (0x4000 + i, (i + 1) as u8)).collect())
    );
    let last = CpuState::from_bytes(returned[1].cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (last.eip, last.instruction_count, last.registers.ecx),
        (0x1002, 1, 0)
    );
    assert_eq!((last.registers.esi, last.registers.edi), (0x300e, 0x4010));
    assert_eq!(
        returned[1].guest,
        Some((0..16).map(|i| (0x4000 + i, (i + 1) as u8)).collect())
    );
}
test_frontends!(rep_chunk_fallback, check_rep_chunk_fallback);

fn check_zero_count(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xaa, 0x40];
    let mut cpu = initial();
    cpu.registers.edi = 0xffffffff;
    let observations = run(engine, frontend, &code, 2, &input(&code, cpu, &[1]));
    assert_eq!(observations[0].0, SLICE_EXHAUSTED);
    assert_eq!(observations[0].1.eip, 0x1002);
    assert_eq!(observations[0].1.instruction_count, 1);
    assert_eq!(observations[0].1.registers, cpu.registers);
}
test_frontends!(zero_count, check_zero_count);

fn check_comparison_fault(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa6]; // REPE CMPSB
    let mut cpu = initial();
    cpu.registers.ecx = 3;
    cpu.registers.esi = 0x2ffe;
    cpu.registers.edi = 0x3ffe;
    cpu.flags.bytes.cf = 1;
    let mut input = input(&code, cpu, &[1, 1, 1]);
    input.guest.push((0x2ffe, vec![7, 8]));
    input.guest.push((0x3ffe, vec![7, 8]));
    let observations = run(engine, frontend, &code, 1, &input);
    for (_, state) in &observations {
        assert_eq!(state.flags, cpu.flags);
    }
    assert_eq!(observations[2].0, (4 << 48) | 0x4000);
    assert_eq!(observations[2].1.registers.ecx, 1);
    assert_eq!(observations[2].1.registers.esi, 0x3000);
    assert_eq!(observations[2].1.registers.edi, 0x4000);
    assert_eq!(observations[2].1.instruction_count, 0);
}
test_frontends!(comparison_fault, check_comparison_fault);

fn check_comparison_completion(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa6];
    let mut cpu = initial();
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x3000;
    let mut input = input(&code, cpu, &[1]);
    input.guest.push((0x2000, vec![2]));
    input.guest.push((0x3000, vec![1]));
    let observations = run(engine, frontend, &code, 1, &input);
    let state = observations[0].1;
    assert_eq!(state.eip, 0x1002);
    assert_eq!(state.instruction_count, 1);
    assert_eq!(state.registers.ecx, 1);
    assert_eq!(state.flags.status_source.left, 2);
    assert_eq!(state.flags.status_source.right, 1);
}
test_frontends!(comparison_completion, check_comparison_completion);

fn check_backward_fill_chunks(engine: Engine) {
    let code = [0xf3, 0xab];
    let mut cpu = initial();
    cpu.registers.ecx = 1026;
    cpu.registers.edi = 0x3008;
    cpu.registers.eax = 0x44332211;
    cpu.flags.bytes.df = 1;
    let input = input(&code, cpu, &[1026]);
    let observation = engine.observe(interpreter(), &input, 1);
    let [Event::Return { outcome, snapshot }] = observation.events.as_slice() else {
        panic!("run resumes chunks internally until exhaustion");
    };
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)])
    );
    let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (
            state.eip,
            state.instruction_count,
            state.registers.ecx,
            state.registers.edi
        ),
        (0x1002, 1, 0, 0x2000)
    );
    assert_eq!(state.flags, cpu.flags);
    assert_eq!(
        snapshot.guest,
        Some(
            (0..4104)
                .map(|i| (0x2004 + i, [0x11, 0x22, 0x33, 0x44][i as usize % 4]))
                .collect()
        )
    );
}
#[test]
fn backward_fill_resumes_chunks_internally() {
    check_backward_fill_chunks(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_backward_fill_resumes_chunks_internally() {
    check_backward_fill_chunks(Engine::V8);
}
