//! RDTSC reads one host counter per execution and preserves its full bit pattern.

use crate::support::{
    blocks::BlockModules,
    encoding::check_length,
    execution::{test_frontends, Frontend},
    machine::{expected, Exit, Image, Step},
    step::{Argument, Engine, Event, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, compile_interpreter, CpuState, ExecutionProfile, SegmentAttributes,
    SegmentProfile, Segments,
};

fn timestamp_result(mut cpu: CpuState, eax: u32, edx: u32, bytes: u32) -> CpuState {
    cpu.registers.eax = eax;
    cpu.registers.edx = edx;
    cpu.eip += bytes;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn transfers(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    let plain = &[0x0f, 0x31][..];
    let prefixed = &[0x66, 0x67, 0x64, 0x0f, 0x31][..];
    for (profile, code, counter, eax, edx) in [
        (SegmentProfile::Flat32.into(), plain, 0_u64, 0, 0),
        (
            SegmentProfile::Flat32.into(),
            plain,
            0xffff_ffff,
            0xffff_ffff,
            0,
        ),
        (SegmentProfile::Flat32.into(), plain, 0x1_0000_0000, 0, 1),
        (
            SegmentProfile::Flat32.into(),
            prefixed,
            0x0123_4567_89ab_cdef,
            0x89ab_cdef,
            0x0123_4567,
        ),
        (
            SegmentProfile::Segmented32.into(),
            plain,
            0x8000_0000_0000_0001,
            1,
            0x8000_0000,
        ),
        (
            SegmentProfile::Segmented16.into(),
            prefixed,
            u64::MAX,
            u32::MAX,
            u32::MAX,
        ),
        (
            ExecutionProfile::Real16,
            plain,
            0x8765_4321_fedc_ba98,
            0xfedc_ba98,
            0x8765_4321,
        ),
        (
            ExecutionProfile::Real16,
            prefixed,
            0xfedc_ba98_7654_3210,
            0x7654_3210,
            0xfedc_ba98,
        ),
    ] {
        let mut image = Image::new(code);
        match profile {
            ExecutionProfile::Real16 => image.cpu.segments = Segments::real_mode(),
            ExecutionProfile::Protected(SegmentProfile::Segmented16) => {
                image.cpu.segments.cs.attributes = SegmentAttributes::from_bits(7);
            }
            _ => {}
        }
        image.cpu.flags.status_source.kind = 9;
        image.cpu.flags.status_source.left = 7;
        image.cpu.flags.status_source.right = 8;
        let mut input = image.input();
        input.timestamp_reads.push(Argument::I64(counter as i64));
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, code, 1, profile),
            Frontend::Interpreter => TestModule::interpreter_with_profile(profile),
        };
        let cpu = timestamp_result(image.cpu, eax, edx, code.len() as u32);
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        wanted.events.insert(0, Event::TimestampRead);
        assert_eq!(
            engine.observe(module, &input, 1),
            wanted,
            "{profile:?}, {counter:#018x}"
        );
    }
}
test_frontends!(full_width_counter_and_preserved_state, transfers);

fn ordering(engine: Engine) {
    // The first result is dead. The second is copied out before the third read.
    let code = [
        0x0f, 0x31, 0xb8, 0, 0, 0, 0, 0x0f, 0x31, 0x89, 0xc1, 0x89, 0xd3, 0x0f, 0x31, 0xeb, 0,
    ];
    let image = Image::new(&code);
    let mut input = image.input();
    input.timestamp_reads = vec![
        Argument::I64(0x1234_5678),
        Argument::I64(0xffff_ffff),
        Argument::I64(0x1_0000_0000),
    ];
    let mut cpu = image.cpu;
    cpu.registers.eax = 0;
    cpu.registers.edx = 1;
    cpu.registers.ecx = u32::MAX;
    cpu.registers.ebx = 0;
    cpu.eip += code.len() as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(7);
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.splice(
        0..0,
        [
            Event::TimestampRead,
            Event::TimestampRead,
            Event::TimestampRead,
        ],
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 20).unwrap());
    let run = TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap());
    for module in [&block, &run] {
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }

    // RDTSC continues to CPUID, which consumes its EAX and ends the entry.
    let code = [0x0f, 0x31, 0x0f, 0xa2, 0xb8, 0, 0, 0, 0];
    let image = Image::new(&code);
    let mut input = image.input();
    input
        .timestamp_reads
        .push(Argument::I64(0x1234_5678_0000_0001));
    input.cpuid_results.push([0, 0, 0, 1 << 4]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0;
    cpu.registers.ebx = 0;
    cpu.registers.ecx = 0;
    cpu.registers.edx = 1 << 4;
    cpu.eip += 4;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(2);
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.splice(
        0..0,
        [
            Event::TimestampRead,
            Event::Cpuid {
                leaf: 1,
                subleaf: image.cpu.registers.ecx,
            },
        ],
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 20).unwrap());
    for module in [&block, &run] {
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }
}

#[test]
fn repeated_reads_remain_observable_without_ending_execution() {
    ordering(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_repeated_reads_remain_observable_without_ending_execution() {
    ordering(Engine::V8);
}

fn fault_order(engine: Engine) {
    let code = [0x0f, 0x31, 0x8b, 0x1d, 0, 0x40, 0, 0];
    let image = Image::new(&code);
    let mut input = image.input();
    input
        .timestamp_reads
        .push(Argument::I64(0x1234_5678_9abc_def0));
    let cpu = timestamp_result(image.cpu, 0x9abc_def0, 0x1234_5678, 2);
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x4000,
                error: 0,
            },
        }],
    );
    wanted.events.insert(0, Event::TimestampRead);
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 2).unwrap());
    let run = TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap());
    for module in [&block, &run] {
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }

    let code = [0x8b, 0x1d, 0, 0x40, 0, 0, 0x0f, 0x31];
    let image = Image::new(&code);
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 2).unwrap());
    for module in [&block, &run] {
        image.check_unchanged_exit(
            engine,
            module,
            "a prior fault prevents the counter read",
            Exit::PageFault {
                address: 0x4000,
                error: 0,
            },
        );
    }
}

#[test]
fn counter_reads_and_faults_keep_instruction_order() {
    fault_order(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_counter_reads_and_faults_keep_instruction_order() {
    fault_order(Engine::V8);
}

fn fetch_boundary(engine: Engine) {
    let mut image = Image::empty();
    image.cpu.eip = 0x1fff;
    image.map(1, 0x3000, false);
    image.data(0x3fff, &[0x0f]);
    image.check_unchanged_exit(
        engine,
        TestModule::interpreter(),
        "RDTSC needs its second byte",
        Exit::PageFault {
            address: 0x2000,
            error: 0x10,
        },
    );

    image.cpu.eip = 0x1ffe;
    image.data(0x3ffe, &[0x0f, 0x31]);
    let mut input = image.input();
    input
        .timestamp_reads
        .push(Argument::I64(0x1234_5678_9abc_def0));
    let cpu = timestamp_result(image.cpu, 0x9abc_def0, 0x1234_5678, 2);
    let run = TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap());
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        }],
    );
    wanted.events.insert(0, Event::TimestampRead);
    assert_eq!(engine.observe(&run, &input, 1), wanted);

    let image = Image::new(&[0xf0, 0x0f, 0x31]);
    image.check_unchanged_exit(
        engine,
        TestModule::interpreter(),
        "LOCK rejects before reading the counter",
        Exit::Other(0x0008_00f0_0000_1000),
    );
}

#[test]
fn interpreter_reads_only_after_complete_supported_encoding() {
    fetch_boundary(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interpreter_reads_only_after_complete_supported_encoding() {
    fetch_boundary(Engine::V8);
}

#[test]
fn encoding_and_imports() {
    for code in [&[0x0f, 0x31][..], &[0x66, 0x67, 0x64, 0x0f, 0x31]] {
        check_length(code);
    }
    for (code, expected) in [(&[0x0f, 0x31][..], true), (&[0x90][..], false)] {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let imports = wasmparser::Parser::new(0)
            .parse_all(&module.bytes)
            .filter_map(|payload| match payload.unwrap() {
                wasmparser::Payload::ImportSection(imports) => Some(imports),
                _ => None,
            })
            .flatten()
            .map(|import| import.unwrap().name)
            .collect::<Vec<_>>();
        assert_eq!(imports.contains(&"readTimestampCounter"), expected);
    }
}
