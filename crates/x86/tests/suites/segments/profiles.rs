use super::data;
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, SegmentProfile};

fn changed_runtime_cache(engine: Engine) {
    let code = [0x64, 0x8b, 0x03];
    let compiled = compile_block_from_bytes(0x1000, &code, 1).unwrap();
    let profile = compiled.segment_profile.unwrap();
    assert_eq!(profile, SegmentProfile::Flat32);
    let block = TestModule::new(&compiled);
    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x20;
    image.map(4, 0x8000, false);
    image.map(5, 0xa000, false);
    image.data(0x8020, &[0x11; 4]);
    image.data(0xa020, &[0x22; 4]);
    for (base, value) in [(0x4000, 0x1111_1111), (0x5000, 0x2222_2222)] {
        image.cpu.segments.fs = data(base, 0xff);
        assert!(profile.is_compatible_with(&image.cpu.segments));
        let mut cpu = image.cpu;
        cpu.registers.eax = value;
        cpu.eip = 0x1003;
        cpu.instruction_count = 0;
        for module in [&block, TestModule::interpreter()] {
            assert_eq!(
                engine.observe(module, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(0x1003)
                    }]
                )
            );
        }
    }
    image.cpu.segments.fs.limit = 0x20;
    assert!(profile.is_compatible_with(&image.cpu.segments));
    for module in [&block, TestModule::interpreter()] {
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 }
                }]
            )
        );
    }
    image.cpu.segments.ds.base = 0x6000;
    assert!(!profile.is_compatible_with(&image.cpu.segments));
    assert!(SegmentProfile::Segmented32.is_compatible_with(&image.cpu.segments));
}

#[test]
fn compiled_entries_reuse_only_compatible_segment_assumptions() {
    changed_runtime_cache(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn compatible_cache_changes_are_read_in_v8() {
    changed_runtime_cache(Engine::V8);
}
