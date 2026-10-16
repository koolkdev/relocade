use super::*;
use crate::support::machine::Exit;
use crate::{DescriptorTable, DescriptorTableRegister};

fn query() -> DescriptorTableQuery {
    DescriptorTableQuery::new(
        DescriptorTable::Global,
        DescriptorTableRegister {
            base: 0xcafe_babe,
            limit: 0xbeef,
        },
    )
}

fn forms(engine: Engine) {
    for (profile, code, bytes) in [
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x0f, 0x01, 0x03][..],
            [0xef, 0xbe, 0xbe, 0xba, 0xfe, 0xca],
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x66, 0x0f, 0x01, 0x03][..],
            [0xef, 0xbe, 0xbe, 0xba, 0xfe, 0],
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x0f, 0x01, 0x07][..],
            [0xef, 0xbe, 0xbe, 0xba, 0xfe, 0],
        ),
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented16),
            &[0x66, 0x0f, 0x01, 0x07][..],
            [0xef, 0xbe, 0xbe, 0xba, 0xfe, 0xca],
        ),
        (
            ExecutionProfile::Real16,
            &[0x0f, 0x01, 0x07][..],
            [0xef, 0xbe, 0xbe, 0xba, 0xfe, 0],
        ),
        (
            ExecutionProfile::Real16,
            &[0x66, 0x0f, 0x01, 0x07][..],
            [0xef, 0xbe, 0xbe, 0xba, 0xfe, 0xca],
        ),
    ] {
        let mut image = image(code, profile);
        image.cpu.registers.ebx = 0x4ffa;
        image.map(4, 0x8000, true);
        image.data(0x8ff9, &[0xa5; 7]);
        let cpu = completed(&image, code.len());
        check_query(
            engine,
            profile,
            code,
            &image,
            Some(Query::DescriptorTable(query())),
            Step {
                cpu,
                ram: &[(0x8ffa, &bytes)],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

fn split_store(engine: Engine) {
    let profile = ExecutionProfile::Protected(SegmentProfile::Segmented32);
    let code = [0x65, 0x67, 0x0f, 0x01, 0x07]; // SGDT GS:[BX].
    let mut image = image(&code, profile);
    image.cpu.registers.ebx = 0xabcd_fffc;
    image.cpu.segments.gs.base = 0x4000;
    image.cpu.segments.gs.limit = 0x10001;
    image.map(0x13, 0x8000, true);
    image.map(0x14, 0xa000, true);
    image.data(0x8ffb, &[0xa5; 5]);
    image.data(0xa000, &[0xa5; 3]);
    let cpu = completed(&image, code.len());
    check_query(
        engine,
        profile,
        &code,
        &image,
        Some(Query::DescriptorTable(query())),
        Step {
            cpu,
            ram: &[(0x8ffc, &[0xef, 0xbe, 0xbe, 0xba]), (0xa000, &[0xfe, 0xca])],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

fn faults(engine: Engine) {
    let profile = ExecutionProfile::Protected(SegmentProfile::Segmented32);
    let code = [0x0f, 0x01, 0x03];
    for (limit, next_page_present, exit) in [
        (0x4fff, false, Exit::GeneralProtection { error: 0 }),
        (
            u32::MAX,
            false,
            Exit::PageFault {
                address: 0x5000,
                error: 2,
            },
        ),
        (
            u32::MAX,
            true,
            Exit::PageFault {
                address: 0x5000,
                error: 3,
            },
        ),
    ] {
        let mut image = image(&code, profile);
        image.cpu.registers.ebx = 0x4ffc;
        image.cpu.segments.ds.limit = limit;
        image.map(4, 0x8000, true);
        image.data(0x8ffc, &[0xa5; 4]);
        if next_page_present {
            image.map(5, 0xa000, false);
        }
        check_query(
            engine,
            profile,
            &code,
            &image,
            None,
            Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            },
        );
    }
}

#[test]
fn operand_sizes_and_modes_store_the_table_register() {
    forms(Engine::Wasmtime);
}

#[test]
fn override_and_split_store_check_all_six_bytes() {
    split_store(Engine::Wasmtime);
    faults(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_table_stores_and_faults() {
    forms(Engine::V8);
    split_store(Engine::V8);
    faults(Engine::V8);
}

fn live_view(engine: Engine) {
    let profile = ExecutionProfile::Protected(SegmentProfile::Flat32);
    let code = [0x0f, 0x01, 0x03, 0xeb, 0xfb]; // SGDT [EBX]; JMP back.
    let mut image = image(&code, profile);
    image.cpu.registers.ebx = 0x4000;
    image.map(4, 0x8000, true);
    let mut input = image.input();
    input.descriptor_table_queries = vec![
        query(),
        DescriptorTableQuery::new(
            DescriptorTable::Global,
            DescriptorTableRegister {
                base: 0x1234_5678,
                limit: 0xffff,
            },
        ),
    ];
    let steps = [
        [0xef, 0xbe, 0xbe, 0xba, 0xfe, 0xca],
        [0xff, 0xff, 0x78, 0x56, 0x34, 0x12],
    ];
    let mut cpu = image.cpu;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(2);
    let first_ram = [(0x8000, steps[0].as_slice())];
    let first = Step {
        cpu,
        ram: &first_ram,
        exit: Exit::Dispatch(cpu.eip),
    };
    cpu.instruction_count = cpu.instruction_count.wrapping_add(2);
    let second_ram = [(0x8000, steps[1].as_slice())];
    let second = Step {
        cpu,
        ram: &second_ram,
        exit: Exit::Dispatch(cpu.eip),
    };
    let mut wanted = expected(&image, &[first, second]);
    wanted
        .events
        .insert(0, Event::QueryDescriptorTable { table: 0 });
    wanted
        .events
        .insert(3, Event::QueryDescriptorTable { table: 0 });
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 2, profile);
    let run = TestModule::new(&crate::compile_interpreter(profile).unwrap());
    for module in [block, &run] {
        assert_eq!(engine.observe(module, &input, 2), wanted);
    }
}

#[test]
fn reused_entries_read_the_current_host_view() {
    live_view(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_reused_entries_read_the_current_host_view() {
    live_view(Engine::V8);
}
