//! CPUID describes the built-in virtual CPU and serializes the next fetch.

use crate::support::{
    blocks::BlockModules,
    encoding::check_length,
    execution::{test_frontends, Frontend},
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, compile_interpreter, CpuState, ExecutionProfile, SegmentAttributes,
    SegmentProfile, Segments,
};

const VENDOR: [u32; 4] = [1, 0x6f6c_6552, 0x5550_4320, 0x6564_6163];
const FEATURES: [u32; 4] = [0x601, 0, 0x0080_0000, 0x0000_8100];

fn result(mut cpu: CpuState, values: [u32; 4], bytes: u32, count: u32) -> CpuState {
    [
        cpu.registers.eax,
        cpu.registers.ebx,
        cpu.registers.ecx,
        cpu.registers.edx,
    ] = values;
    cpu.eip += bytes;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(count);
    cpu
}

fn forms(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for profile in [
        SegmentProfile::Flat32.into(),
        SegmentProfile::Segmented32.into(),
        SegmentProfile::Segmented16.into(),
        ExecutionProfile::Real16,
    ] {
        for (code, leaf, subleaf, values) in [
            (&[0x0f, 0xa2][..], 0, u32::MAX, VENDOR),
            (&[0x66, 0x67, 0x64, 0x0f, 0xa2], 1, 0x8000_0001, FEATURES),
            (&[0x0f, 0xa2], 0x0001_0000, 0, FEATURES),
        ] {
            let mut image = Image::new(code);
            match profile {
                ExecutionProfile::Real16 => image.cpu.segments = Segments::real_mode(),
                ExecutionProfile::Protected(SegmentProfile::Segmented16) => {
                    image.cpu.segments.cs.attributes = SegmentAttributes::from_bits(7);
                }
                _ => {}
            }
            image.cpu.registers.eax = leaf;
            image.cpu.registers.ecx = subleaf;
            image.cpu.flags.status_source.kind = 9;
            image.cpu.flags.status_source.left = 7;
            image.cpu.flags.status_source.right = 8;
            image.cpu.flags.bytes.id = (leaf != 0) as u8;
            let input = image.input();
            let module = match frontend {
                Frontend::Block => blocks.get(&image.cpu, code, 8, profile),
                Frontend::Interpreter => TestModule::interpreter_with_profile(profile),
            };
            let cpu = result(image.cpu, values, code.len() as u32, 1);
            let wanted = expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
            assert_eq!(
                engine.observe(module, &input, 1),
                wanted,
                "{profile:?}, {code:02x?}"
            );
        }
    }
}
test_frontends!(full_register_queries_preserve_other_state, forms);

fn serialization(engine: Engine) {
    // Rewrite the immediate after CPUID. Its old bytes must not execute.
    let code = [
        0xc6, 0x05, 0x0a, 0x10, 0, 0, 0x77, 0x0f, 0xa2, 0xbb, 0x11, 0, 0, 0, 0xeb, 0,
    ];
    let mut image = Image::new(&code);
    image.map(1, 0x3000, true);
    let input = image.input();
    let cpu = result(image.cpu, FEATURES, 9, 2);
    let first = Step {
        cpu,
        ram: &[(0x300a, &[0x77])],
        exit: Exit::Dispatch(cpu.eip),
    };
    let wanted = expected(&image, &[first]);
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 16).unwrap());
    let run = TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap());
    for module in [&block, &run] {
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }

    let mut resumed = cpu;
    resumed.registers.ebx = 0x77;
    resumed.eip = 0x1010;
    resumed.instruction_count = resumed.instruction_count.wrapping_add(2);
    let wanted = expected(
        &image,
        &[
            Step {
                cpu,
                ram: &[(0x300a, &[0x77])],
                exit: Exit::Dispatch(cpu.eip),
            },
            Step {
                cpu: resumed,
                ram: &[],
                exit: Exit::Dispatch(resumed.eip),
            },
        ],
    );
    assert_eq!(engine.observe(&run, &input, 2), wanted);
}

#[test]
fn serialization_dispatches_before_modified_code() {
    serialization(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_serialization_dispatches_before_modified_code() {
    serialization(Engine::V8);
}

fn fetch_boundary(engine: Engine) {
    let mut image = Image::empty();
    image.cpu.eip = 0x1ffe;
    image.map(1, 0x3000, false);
    image.data(0x3ffe, &[0x0f, 0xa2]);
    let input = image.input();
    let cpu = result(image.cpu, FEATURES, 2, 1);
    let wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    let run = TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap());
    assert_eq!(engine.observe(&run, &input, 1), wanted);

    image.cpu.eip = 0x1fff;
    image.data(0x3fff, &[0x0f]);
    image.check_unchanged_exit(
        engine,
        &run,
        "CPUID requires its second byte",
        Exit::PageFault {
            address: 0x2000,
            error: 0x10,
        },
    );

    let image = Image::new(&[0xf0, 0x0f, 0xa2]);
    image.check_unchanged_exit(
        engine,
        TestModule::interpreter(),
        "LOCK rejects CPUID without changing state",
        Exit::Other(0x0008_00f0_0000_1000),
    );
}

#[test]
fn queries_follow_complete_fetch_and_stop_before_successor_fetch() {
    fetch_boundary(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_queries_follow_complete_fetch_and_stop_before_successor_fetch() {
    fetch_boundary(Engine::V8);
}

#[test]
fn encoding_and_import_boundaries() {
    for code in [&[0x0f, 0xa2][..], &[0x66, 0x67, 0x64, 0x0f, 0xa2]] {
        let one = check_length(code);
        let suffix = [code, &[0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &suffix, 16).unwrap().bytes,
            one.bytes
        );
    }
    for code in [&[0x0f, 0xa2][..], &[0x90][..]] {
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
        assert!(!imports.contains(&"cpuid"));
    }
}

fn leaf_selection(engine: Engine) {
    let run = TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap());
    for (leaf, wanted_values) in [
        (0, VENDOR),
        (1, FEATURES),
        (2, FEATURES),
        (0x8000_0000, FEATURES),
        (0x8000_0001, FEATURES),
        (u32::MAX, FEATURES),
    ] {
        // CPUID consumes the preceding register definition in the same block.
        let mut code = vec![0xb8];
        code.extend(leaf.to_le_bytes());
        code.extend([0x0f, 0xa2]);
        let image = Image::new(&code);
        let cpu = result(image.cpu, wanted_values, 7, 2);
        let wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 2).unwrap());
        for module in [&block, &run] {
            assert_eq!(
                engine.observe(module, &image.input(), 1),
                wanted,
                "leaf {leaf:#x}"
            );
        }
    }
}

#[test]
fn leaf_selection_uses_current_full_width_eax() {
    leaf_selection(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_leaf_selection_uses_current_full_width_eax() {
    leaf_selection(Engine::V8);
}
