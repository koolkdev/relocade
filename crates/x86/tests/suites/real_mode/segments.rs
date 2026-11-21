use super::*;

fn loads(frontend: Frontend) {
    let mut cases = sequences(frontend);
    for (segment, modrm, value) in [
        (Segment::Es, 0xc0, 0),
        (Segment::Ss, 0xd0, 3),
        (Segment::Ds, 0xd8, 0xffff),
        (Segment::Fs, 0xe0, 0x1234),
        (Segment::Gs, 0xe8, 4),
    ] {
        let code = [0x8e, modrm];
        let mut image = image(&code);
        image.cpu.registers.eax = 0xabcd_0000 | u32::from(value);
        let mut cpu = retired(&image, code.len());
        cpu.segments[segment] = cache(segment, value);
        cases.check(
            "MOV segment accepts all low-word bits without a resolver",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: if segment == Segment::Ss {
                    Exit::Other(crate::ExecutionExit::InterruptInhibited)
                } else {
                    Exit::Dispatch(cpu.eip)
                },
            }],
        );
    }

    let code = [0x8e, 0x1e, 0x00, 0x20]; // MOV DS,[2000]
    let mut image = image(&code);
    image.cpu.segments.ds = cache(Segment::Ds, 0x300);
    image.map(5, 0x8000, false);
    image.data(0x8000, &[0, 0]);
    let mut cpu = retired(&image, code.len());
    cpu.segments.ds = cache(Segment::Ds, 0);
    cases.check(
        "memory source uses the old DS before loading zero",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}
test_frontends!(segment_loads, loads);

fn stack_and_pointers(frontend: Frontend) {
    let mut cases = sequences(frontend);
    let code = [0x17]; // POP SS
    let mut image = image(&code);
    image.cpu.segments.ss = cache(Segment::Ss, 0x300);
    image.cpu.registers.esp = 0xabcd_0010;
    image.map(3, 0x8000, false);
    image.data(0x8010, &[0, 0]);
    let mut cpu = retired(&image, code.len());
    cpu.registers.esp = 0xabcd_0012;
    cpu.segments.ss = cache(Segment::Ss, 0);
    cases.check(
        "POP SS reads and advances the old stack",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Other(crate::ExecutionExit::InterruptInhibited),
        }],
    );

    for (code, segment, value) in [
        (&[0xc5, 0x06, 0, 0x20][..], Segment::Ds, 0x1111_5678),
        (
            &[0x66, 0x0f, 0xb2, 0x06, 0, 0x20][..],
            Segment::Ss,
            0x1234_5678,
        ),
    ] {
        let mut image = super::image(code);
        image.map(2, 0x8000, false);
        let pointer = if segment == Segment::Ds {
            &[0x78, 0x56, 0, 0][..]
        } else {
            &[0x78, 0x56, 0x34, 0x12, 0xff, 0xff][..]
        };
        image.data(0x8000, pointer);
        let mut cpu = retired(&image, code.len());
        cpu.registers.eax = value;
        cpu.segments[segment] = cache(segment, if segment == Segment::Ds { 0 } else { 0xffff });
        cases.check(
            "far pointer loads use operand width and ordinary segment values",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }

    let code = [0xc5, 0x06, 0xfe, 0xff];
    let image = super::image(&code);
    cases.check(
        "the whole far pointer must fit before AX or DS changes",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
}
test_frontends!(stack_and_pointer_loads, stack_and_pointers);

#[test]
fn pc_stops_protected_descriptor_access_before_reading_mmio() {
    use crate::support::{machine::expected, step::TestModule};
    use relocade_x86::SegmentDefaultSize::Bits16;

    let pc = TestModule::from_bytes(relocade_interpreter::pc(), crate::abi::STEP.name, None);
    for code in [
        &[0x8e, 0x1e, 0x00, 0x20][..],   // MOV DS,[2000h]
        &[0xc4, 0x06, 0x00, 0x20],       // LES AX,[2000h]
        &[0xff, 0x1e, 0x00, 0x20],       // CALL FAR [2000h]
        &[0xcb],                         // RETF from the MMIO stack
        &[0x0f, 0x02, 0x06, 0x00, 0x20], // LAR AX,[2000h]
        &[0x0f, 0x00, 0x26, 0x00, 0x20], // VERR [2000h]
    ] {
        let mut image = Image::new(code);
        image.cpu = CpuState {
            eip: image.cpu.eip,
            ..CpuState::protected_mode(Bits16, Bits16)
        };
        image.cpu.registers.esp = 0x2000;
        let mut input = image.input();
        input.mmio_pages = vec![(2, 0x8000)];
        input.observe_mmio = true;
        assert_eq!(
            pc.observe(&input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::UnsupportedInstruction {
                        eip: image.cpu.eip,
                        opcode: code[0],
                    },
                }]
            ),
            "{code:02x?}"
        );
    }
}
