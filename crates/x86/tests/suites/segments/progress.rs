use super::data;
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, SegmentProfile};

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

fn code_boundaries(engine: Engine) {
    let module = TestModule::interpreter_with_profile(SegmentProfile::Segmented32);
    for bytes in [&[0x90][..], &[0x74, 0x7f]] {
        let mut image = Image::empty();
        image.cpu.flags.status_source.kind = 0;
        image.cpu.flags.bytes.zf = 0;
        image.cpu.segments.cs = super::code(0x8000, 0x1000 + bytes.len() as u32 - 1);
        image.map(9, 0x3000, false);
        image.data(0x3000, bytes);
        let mut cpu = image.cpu;
        cpu.eip += bytes.len() as u32;
        cpu.instruction_count = 0;
        assert_eq!(
            engine.observe(module, &image.input(), 2),
            expected(
                &image,
                &[
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip)
                    },
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::GeneralProtection { error: 0 }
                    },
                ]
            )
        );
    }

    // CALL and RET retire before a page fault fetching their legal CS target.
    for is_call in [true, false] {
        let mut image = Image::empty();
        image.cpu.segments.cs = super::code(0x8000, 0x2000);
        image.map(9, 0x3000, false);
        image.map(4, 0x6000, true);
        image.cpu.registers.esp = if is_call { 0x4004 } else { 0x4000 };
        image.data(
            0x3000,
            if is_call {
                &[0xe8, 0xfb, 0x0f, 0, 0]
            } else {
                &[0xc3]
            },
        );
        image.data(0x6000, &0x2000u32.to_le_bytes());
        let mut cpu = image.cpu;
        cpu.eip = 0x2000;
        cpu.instruction_count = 0;
        cpu.registers.esp = if is_call { 0x4000 } else { 0x4004 };
        let ram: &[(u32, &[u8])] = if is_call {
            &[(0x6000, &[0x05, 0x10, 0, 0])]
        } else {
            &[]
        };
        assert_eq!(
            engine.observe(module, &image.input(), 2),
            expected(
                &image,
                &[
                    Step {
                        cpu,
                        ram,
                        exit: Exit::Dispatch(0x2000)
                    },
                    Step {
                        cpu,
                        ram,
                        exit: Exit::PageFault {
                            address: 0xa000,
                            error: 0x10
                        }
                    },
                ]
            )
        );
    }

    // MOV EAX,1; ADD EAX,1; LOOP beyond CS. Only the first two instructions retire.
    let mut image = Image::empty();
    image.cpu.registers.ecx = 2;
    image.cpu.segments.cs = super::code(0x8000, 0x1009);
    image.map(9, 0x3000, false);
    image.data(0x3000, &[0xb8, 1, 0, 0, 0, 0x83, 0xc0, 1, 0xe2, 0x16]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 1;
    cpu.eip = 0x1005;
    cpu.instruction_count = 0;
    let first = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    };
    cpu.registers.eax = 2;
    cpu.eip = 0x1008;
    cpu.instruction_count = 1;
    cpu.flags.status_source.kind = 10;
    cpu.flags.status_source.left = 1;
    cpu.flags.status_source.right = 1;
    assert_eq!(
        engine.observe(module, &image.input(), 3),
        expected(
            &image,
            &[
                first,
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(0x1008)
                },
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 }
                },
            ]
        )
    );
}

#[test]
fn cs_faults_preserve_the_correct_instruction_boundary() {
    code_boundaries(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_cs_faults_preserve_the_correct_instruction_boundary() {
    code_boundaries(Engine::V8);
}
