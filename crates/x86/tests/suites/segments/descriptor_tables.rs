use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    DescriptorTables, Segment, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind,
    SegmentLimit, SegmentProfile,
};

fn loaded_cache_lifetime(engine: Engine) {
    let code = [0x64, 0x8b, 0x03]; // MOV EAX,FS:[EBX].
    let mut tables = DescriptorTables::default();
    let descriptor = SegmentDescriptor::new(
        0x6000,
        SegmentLimit::bytes(0xffff).unwrap(),
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        SegmentDefaultSize::Bits32,
    );
    tables.insert(0x37, descriptor);
    let old = tables.resolve_user_segment(Segment::Fs, 0x37).unwrap();
    tables.insert(
        0x34,
        SegmentDescriptor {
            base: 0xa000,
            ..descriptor
        },
    );
    let reloaded = tables.resolve_user_segment(Segment::Fs, 0x37).unwrap();
    tables.remove(0x36);

    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x200;
    image.map(6, 0x8000, false);
    image.map(10, 0x9000, false);
    image.data(0x8200, &[0x44, 0x33, 0x22, 0x11]);
    image.data(0x9200, &[0x88, 0x77, 0x66, 0x55]);
    let profile = SegmentProfile::Segmented32;
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 1, profile);
    for (fs, value) in [(old, 0x1122_3344), (reloaded, 0x5566_7788)] {
        image.cpu.segments.fs = fs;
        let mut cpu = image.cpu;
        cpu.registers.eax = value;
        cpu.eip = 0x1003;
        cpu.instruction_count = 0;
        for (name, module) in [
            ("block", block),
            ("interpreter", TestModule::interpreter_with_profile(profile)),
        ] {
            assert_eq!(
                engine.observe(module, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(0x1003)
                    }]
                ),
                "{name} uses loaded FS base {:#x} after its table entry was removed",
                fs.base,
            );
        }
    }
}

#[test]
fn table_changes_take_effect_only_when_a_segment_is_reloaded() {
    loaded_cache_lifetime(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_table_changes_take_effect_only_when_a_segment_is_reloaded() {
    loaded_cache_lifetime(Engine::V8);
}
