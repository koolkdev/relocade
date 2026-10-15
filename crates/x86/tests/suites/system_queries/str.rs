use super::*;
use crate::support::machine::Exit;
use crate::SystemSelector;

fn forms(engine: Engine) {
    for (profile, code, eax) in [
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x0f, 0x00, 0xc8][..],
            0xf330,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x66, 0x0f, 0x00, 0xc8][..],
            0xabcd_f330,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x0f, 0x00, 0xc8][..],
            0xabcd_f330,
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x66, 0x0f, 0x00, 0xc8][..],
            0xf330,
        ),
    ] {
        let mut image = image(code, profile);
        image.cpu.registers.eax = 0xabcd_ffff;
        let mut cpu = completed(&image, code.len());
        cpu.registers.eax = eax;
        check_query(
            engine,
            profile,
            code,
            &image,
            Some(Query::SystemSelector(SystemSelectorQuery::new(
                SystemSelector::Tr,
                0xf330,
            ))),
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
    let profile = ExecutionProfile::Protected(SegmentProfile::Flat32);
    let code = [0x0f, 0x00, 0x0b]; // STR [EBX].
    let mut image = image(&code, profile);
    image.cpu.registers.ebx = 0x4ffe;
    image.map(4, 0x8000, true);
    image.data(0x8ffd, &[0xa5; 3]);
    let cpu = completed(&image, code.len());
    check_query(
        engine,
        profile,
        &code,
        &image,
        Some(Query::SystemSelector(SystemSelectorQuery::new(
            SystemSelector::Tr,
            0xf330,
        ))),
        Step {
            cpu,
            ram: &[(0x8ffe, &[0x30, 0xf3])],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

fn real_mode(engine: Engine) {
    let profile = ExecutionProfile::Real16;
    for code in [&[0x0f, 0x00, 0xc8][..], &[0x0f, 0x00, 0x0e, 0xff, 0xff][..]] {
        let image = image(code, profile);
        check_query(
            engine,
            profile,
            code,
            &image,
            None,
            Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::InvalidOpcode,
            },
        );
    }
}

fn live_selectors(engine: Engine) {
    let profile = ExecutionProfile::Protected(SegmentProfile::Flat32);
    let code = [0x0f, 0x00, 0xc0, 0x0f, 0x00, 0xca, 0xeb, 0xf8]; // SLDT EAX; STR EDX; JMP back.
    let image = image(&code, profile);
    let mut input = image.input();
    input.system_selector_queries = vec![
        SystemSelectorQuery::new(SystemSelector::Ldtr, 0x28),
        SystemSelectorQuery::new(SystemSelector::Tr, 0xf330),
        SystemSelectorQuery::new(SystemSelector::Ldtr, 0),
        SystemSelectorQuery::new(SystemSelector::Tr, 0x70),
    ];
    let mut cpu = image.cpu;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(3);
    cpu.registers.eax = 0x28;
    cpu.registers.edx = 0xf330;
    let first = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    };
    cpu.instruction_count = cpu.instruction_count.wrapping_add(3);
    cpu.registers.eax = 0;
    cpu.registers.edx = 0x70;
    let second = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    };
    let mut wanted = expected(&image, &[first, second]);
    for offset in [0, 4] {
        wanted.events.splice(
            offset..offset,
            [
                Event::QuerySystemSelector { register: 0 },
                Event::QuerySystemSelector { register: 1 },
            ],
        );
    }
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 3, profile);
    let run = TestModule::new(&crate::compile_interpreter(profile).unwrap());
    for module in [block, &run] {
        assert_eq!(engine.observe(module, &input, 2), wanted);
    }
}

#[test]
fn operand_forms_store_tr_with_p4_register_widths() {
    forms(Engine::Wasmtime);
}

#[test]
fn real_mode_rejects_before_query_or_operand_access() {
    real_mode(Engine::Wasmtime);
}

#[test]
fn ldtr_and_tr_remain_distinct_between_entries() {
    live_selectors(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_task_register_and_live_system_selectors() {
    forms(Engine::V8);
    real_mode(Engine::V8);
    live_selectors(Engine::V8);
}
