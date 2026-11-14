use crate::support::{
    encoding::{check_length, check_unsupported_at_page_end},
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
};
use relocade_x86::{ExecutionProfile, Gpr32, Segment, SegmentProfile, SegmentRegister};

fn register_results(frontend: Frontend) {
    for (profile, code, register, value) in [
        (
            SegmentProfile::Flat32.into(),
            &[0x0f, 0x01, 0xe0][..],
            Gpr32::Eax,
            0x31,
        ),
        (
            SegmentProfile::Flat32.into(),
            &[0x66, 0x0f, 0x01, 0xe6],
            Gpr32::Esi,
            0x7777_0031,
        ),
        (
            ExecutionProfile::Real16,
            &[0x0f, 0x01, 0xe3],
            Gpr32::Ebx,
            0x4444_0030,
        ),
        (
            ExecutionProfile::Real16,
            &[0x66, 0x0f, 0x01, 0xe4],
            Gpr32::Esp,
            0x30,
        ),
    ] {
        let mut image = Image::new(code);
        if profile == ExecutionProfile::Real16 {
            image.initialize_real_mode();
        }
        let mut cpu = image.cpu;
        cpu.registers[register] = value;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        ImageSequences::new(frontend, profile).check(
            "read the mode's CR0 word and preserve flags and other registers",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}
test_frontends!(register_widths_and_modes, register_results);

fn memory_results(frontend: Frontend) {
    for (profile, status) in [
        (SegmentProfile::Segmented32.into(), 0x31),
        (ExecutionProfile::Real16, 0x30),
    ] {
        let mut checks = ImageSequences::new(frontend, profile);
        for operand_override in [false, true] {
            let real = profile == ExecutionProfile::Real16;
            let mut code = if operand_override { vec![0x66] } else { vec![] };
            code.extend_from_slice(if real {
                &[0x0f, 0x01, 0x66, 0]
            } else {
                &[0x0f, 0x01, 0x23]
            });
            let mut image = Image::new(&code);
            if real {
                image.initialize_real_mode();
                image.cpu.segments.ss = SegmentRegister::real_mode(Segment::Ss, 0x400);
                image.cpu.registers.ebp = 0xaaaa_0ffe;
            } else {
                image.cpu.segments.ds.limit = 0x4fff;
                image.cpu.registers.ebx = 0x4ffe;
            }
            image.map(4, 0x8000, true);
            image.data(0x8ffd, &[0xa5; 3]);
            let mut cpu = image.cpu;
            cpu.eip += code.len() as u32;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            checks.check(
                "memory receives exactly two bytes even with a dword operand size",
                &code,
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x8ffe, &[status, 0])],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}
test_frontends!(memory_is_always_word_sized, memory_results);

fn destination_faults(frontend: Frontend) {
    let code = [0x0f, 0x01, 0x23];
    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x4fff;
    image.map(4, 0x8000, true);
    image.data(0x8fff, &[0xa5]);
    ImageSequences::new(frontend, SegmentProfile::Flat32).check(
        "the second byte faults before any destination or CPU change",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 2,
            },
        }],
    );

    let code = [0x0f, 0x01, 0x66, 0]; // Real mode SMSW [BP].
    let mut image = Image::new(&code);
    image.initialize_real_mode();
    image.cpu.registers.ebp = 0xaaaa_ffff;
    image.map(15, 0x8000, true);
    image.data(0x8fff, &[0xa5]);
    ImageSequences::new(frontend, ExecutionProfile::Real16).check(
        "the word cannot wrap past the real-mode stack segment limit",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::StackFault { error: 0 },
        }],
    );
}
test_frontends!(checks_the_complete_word, destination_faults);

#[test]
fn encoding_rejects_lock_for_registers_and_memory() {
    check_length(&[0x0f, 0x01, 0xe0]);
    check_length(&[0x66, 0x0f, 0x01, 0x64, 0x8b, 0x7f]);
    check_unsupported_at_page_end(&[0xf0, 0x0f, 0x01, 0xe0], 0xf0);
    check_unsupported_at_page_end(&[0xf0, 0x0f, 0x01, 0x23], 0xf0);
}
