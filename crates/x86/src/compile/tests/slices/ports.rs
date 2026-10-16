use super::*;

fn check_port_remapping(engine: Engine, frontend: Frontend) {
    use crate::support::step::DeviceUpdate;
    for opcode in [0x6c, 0x6e] {
        let code = [0xf3, opcode];
        let mut cpu = initial();
        cpu.segments = crate::Segments::real_mode();
        cpu.registers.ecx = 2;
        cpu.registers.edx = 0x80;
        cpu.registers.esi = 0x2000;
        cpu.registers.edi = 0x2000;
        let mut input = Input::new(&cpu.to_bytes());
        input.budgets = vec![2];
        input.observe_guest = true;
        input.physical_pages = vec![(1, 0x1000, true), (2, 0x2000, true)];
        input.guest = vec![
            (0x1000, code.to_vec()),
            (0x2000, vec![0x11, 0x12]),
            (0x3000, vec![0x21, 0x22]),
        ];
        input.port_updates = vec![DeviceUpdate {
            map: vec![(20, 0x3000u32.to_le_bytes().to_vec())],
            ..DeviceUpdate::default()
        }];
        if opcode == 0x6c {
            input.port_reads = vec![0x31, 0x32];
        }
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
        let observation = engine.observe(module, &input, 1);
        let Some(Event::Return { snapshot, .. }) = observation.events.last() else {
            panic!("expected return")
        };
        let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
        assert_eq!(
            (state.eip, state.instruction_count, state.registers.ecx),
            (0x1002, 1, 0)
        );
        if opcode == 0x6c {
            assert_eq!(snapshot.guest, Some(vec![(0x3000, 0x31), (0x3001, 0x32)]));
        } else {
            let values: Vec<_> = observation
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::PortWrite { value, .. } => Some(*value),
                    _ => None,
                })
                .collect();
            assert_eq!(values, vec![0x11, 0x22]);
            assert!(observation.guest_unchanged);
        }
    }
}
test_frontends!(port_remapping, check_port_remapping);

fn check_zero_count_port_rep(engine: Engine, frontend: Frontend) {
    for opcode in [0x6c, 0x6e] {
        let code = [0xf3, opcode];
        let mut cpu = initial();
        cpu.segments = crate::Segments::real_mode();
        let mut input = Input::new(&cpu.to_bytes());
        input.budgets = vec![1];
        input.physical_pages = vec![(1, 0x1000, true)];
        input.guest = vec![(0x1000, code.to_vec())];
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
        let observation = engine.observe(module, &input, 1);
        assert!(observation
            .events
            .iter()
            .all(|event| matches!(event, Event::Return { .. } | Event::Dispatch { .. })));
        let Some(Event::Return { snapshot, .. }) = observation.events.last() else {
            panic!("expected return")
        };
        let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
        assert_eq!(
            (state.eip, state.instruction_count, state.registers.ecx),
            (0x1002, 1, 0)
        );
    }
}
test_frontends!(zero_count_port_rep, check_zero_count_port_rep);
