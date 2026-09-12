use crate::support::cases::{
    test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case, Permissions::ReadWrite,
};
use crate::support::machine;
use crate::support::step;
use machine::{check, Exit, Image, Step};
use step::TestModule;
use wasm86_x86::StoredStatusSource;
use wasm86_x86::{CpuState, Gpr32, StoredFlags};

fn image(start: u32, code: &[u8]) -> Image {
    let mut image = Image::new(&[]);
    image.guest.clear();
    image.data(0x3000 + (start & 0xfff), code);
    image.cpu.eip = start;
    image.cpu.registers.ebx = 0x8000;
    image.cpu.registers.esp = 0x8000;
    image.cpu.registers.edi = 0x8000;
    image.map(8, 0x5000, true);
    image.data(0x507f, &[0xa5; 4]);
    image
}

#[test]
fn missing_immediates_fault_after_conditional_address_fields() {
    let module = TestModule::interpreter();
    for (name, start, code) in [
        (
            "missing byte immediate after SIB and displacement",
            0x1ffc,
            &[0xc6, 0x44, 0x24, 0x7f][..],
        ),
        (
            "wide immediate extends beyond the five-byte proof",
            0x1ffb,
            &[0xc7, 0x44, 0x24, 0x7f, 0x12][..],
        ),
    ] {
        let expected_cpu = image(start, code).cpu;
        check(
            module,
            name,
            &image(start, code),
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            }],
        );
    }
}

fn complete_operand_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, start, code) in [
        (
            "SIB and displacement fill the five-byte window",
            0x1ffb,
            &[0xc6, 0x44, 0x24, 0x7f, 0x80][..],
        ),
        (
            "absent SIB leaves a complete four-byte instruction",
            0x1ffc,
            &[0xc6, 0x43, 0x7f, 0x80][..],
        ),
    ] {
        cases.push(
            Case::preserving_flags(name, code)
                .at(start)
                .initial_registers(&[
                    (Gpr32::Ebx, 0x8000),
                    (Gpr32::Esp, 0x8000),
                    (Gpr32::Edi, 0x8000),
                ])
                .map_page(8, 0x5000, ReadWrite)
                .backing(0x507f, &[0xa5; 4])
                .expect_memory(0x807f, &[0x80]),
        );
    }
    for (name, start) in [
        ("extended opcode retains its direct window", 0x1ffa),
        ("extended opcode uses checked reads at page end", 0x1ffc),
    ] {
        cases.push(
            Case::new(
                name,
                &[0x0f, 0x94, 0x47, 0x7f],
                Flags {
                    cf: false,
                    pf: true,
                    af: false,
                    zf: true,
                    sf: false,
                    of: false,
                },
                Flags::all(Preserved),
            )
            .stored_flags(StoredFlags {
                status_source: StoredStatusSource {
                    kind: 11,
                    left: 0,
                    ..(CpuState::filled(0xa5).flags).status_source
                },
                ..CpuState::filled(0xa5).flags
            })
            .preserve_flag_record()
            .at(start)
            .initial_registers(&[
                (Gpr32::Ebx, 0x8000),
                (Gpr32::Esp, 0x8000),
                (Gpr32::Edi, 0x8000),
            ])
            .map_page(8, 0x5000, ReadWrite)
            .backing(0x507f, &[0xa5; 4])
            .expect_memory(0x807f, &[1]),
        );
    }
    cases
}
test_cases!(
    complete_conditional_fields_at_fetch_boundaries,
    complete_operand_cases()
);
