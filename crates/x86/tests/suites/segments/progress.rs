use super::data;
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::compile_block_from_bytes;

fn check_progress(engine: Engine) {
    // MOV EAX,1; ADD EAX,1; MOV [EBX],EAX; MOV ECX,FS:[EDX].
    let code = [
        0xb8, 1, 0, 0, 0, 0x83, 0xc0, 1, 0x89, 0x03, 0x64, 0x8b, 0x0a,
    ];
    let mut image = Image::new(&code);
    image.cpu.instruction_count = 7;
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.edx = 2;
    image.cpu.segments.fs = data(0x8000, 3);
    image.map(4, 0xc000, true);
    image.data(0xc000, &[0xff; 4]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 1;
    cpu.eip = 0x1005;
    cpu.instruction_count = 8;
    let first = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    };
    cpu.registers.eax = 2;
    cpu.flags.status_source.kind = 10;
    cpu.flags.status_source.left = 1;
    cpu.flags.status_source.right = 1;
    cpu.eip = 0x1008;
    cpu.instruction_count = 9;
    let second = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1008),
    };
    cpu.eip = 0x100a;
    cpu.instruction_count = 10;
    let ram: &[(u32, &[u8])] = &[(0xc000, &[2, 0, 0, 0])];
    let third = Step {
        cpu,
        ram,
        exit: Exit::Dispatch(0x100a),
    };
    let fourth = Step {
        cpu,
        ram,
        exit: Exit::GeneralProtection { error: 0 },
    };
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 4),
        expected(&image, &[first, second, third, fourth])
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 4).unwrap());
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram,
                exit: Exit::GeneralProtection { error: 0 }
            }]
        )
    );
}

#[test]
fn segment_fault_publishes_prior_arithmetic_store_and_count() {
    check_progress(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn segment_fault_publishes_prior_progress_in_v8() {
    check_progress(Engine::V8);
}
