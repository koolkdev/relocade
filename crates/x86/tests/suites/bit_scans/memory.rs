use super::{EVEN, ODD, ZERO};
use crate::support::cases::{
    test_cases, InstructionCase as Case, Permissions::ReadOnly, RegisterExpectation::Exact,
};
use wasm86_x86::Gpr32;

fn source_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, length, addresses, sources) in [
        (
            &[0x66][..],
            2,
            &[0x4001, 0x4ffe, 0x4fff, 0xffff_fffe][..],
            [
                (0_u32, 0x4433_a55b, 0x4433_a55b, ZERO),
                (1, 0x4433_0000, 0x4433_0000, ODD),
                (0x8008, 0x4433_0003, 0x4433_000f, EVEN),
            ],
        ),
        (
            &[][..],
            4,
            &[0x4001, 0x4ffc, 0x4ffd, 0x4ffe, 0x4fff, 0xffff_fffc][..],
            [
                (0, 0x4433_a55b, 0x4433_a55b, ZERO),
                (1, 0, 0, ODD),
                (0x8000_0008, 3, 31, EVEN),
            ],
        ),
    ] {
        for &address in addresses {
            for (source, first, last, flags) in sources {
                for (opcode, result) in [(0xbc, first), (0xbd, last)] {
                    let offset = address & 0xfff;
                    let first_length = length.min((0x1000 - offset) as usize);
                    let bytes = source.to_le_bytes();
                    let mut case = Case::replacing_flags(format!("opcode {opcode:x}, prefix {prefix:02x?}, source {source:x} at {address:x}"),
                        &[prefix, &[0x0f, opcode, 0x03]].concat(), flags)
                        .register(Gpr32::Eax, 0x4433_a55b, result).initial_register(Gpr32::Ebx, address)
                        .map_page(address >> 12, 0x8000, ReadOnly)
                        .backing(0x8000 + offset - 1, &[0x5a])
                        .backing(0x8000 + offset, &bytes[..first_length]);
                    if first_length < length {
                        case = case
                            .map_page((address >> 12) + 1, 0xa000, ReadOnly)
                            .backing(0xa000, &bytes[first_length..length])
                            .backing(0xa000 + (length - first_length) as u32, &[0x5a]);
                    } else if offset + (first_length as u32) < 0x1000 {
                        case = case.backing(0x8000 + offset + first_length as u32, &[0x5a]);
                    }
                    cases.push(case);
                }
            }
        }
    }
    cases
}
test_cases!(readonly_and_scattered_sources, source_cases());

fn address_alias_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (
        name,
        prefix,
        destination,
        registers,
        address_bytes,
        address,
        bytes,
        zero,
        first,
        last,
        flags,
    ) in [
        (
            "EAX is the base",
            &[][..],
            Gpr32::Eax,
            &[(Gpr32::Eax, 0x4000)][..],
            &[0x00][..],
            0x4000,
            &[8, 0x80, 0, 0x80][..],
            0x4000,
            3,
            31,
            ODD,
        ),
        (
            "AX preserves the upper half of its full EAX base",
            &[0x66][..],
            Gpr32::Eax,
            &[(Gpr32::Eax, 0x8000_4020)][..],
            &[0x00][..],
            0x8000_4020,
            &[8, 0x80][..],
            0x8000_4020,
            0x8000_0003,
            0x8000_000f,
            EVEN,
        ),
        (
            "ECX is a wrapping scaled index",
            &[][..],
            Gpr32::Ecx,
            &[(Gpr32::Ebx, 0x4010), (Gpr32::Ecx, 0x4000_0001)][..],
            &[0x4c, 0x8b, 0xfc][..],
            0x4010,
            &[8, 0x80, 0, 0x80][..],
            0x4000_0001,
            3,
            31,
            ODD,
        ),
        (
            "ESP is the SIB base",
            &[][..],
            Gpr32::Esp,
            &[(Gpr32::Esp, 0x4000)][..],
            &[0x24, 0x24][..],
            0x4000,
            &[8, 0x80, 0, 0x80][..],
            0x4000,
            3,
            31,
            ODD,
        ),
    ] {
        for (source_bytes, first, last, flags) in [
            (vec![0; bytes.len()], zero, zero, ZERO),
            (bytes.to_vec(), first, last, flags),
        ] {
            for (opcode, result) in [(0xbc, first), (0xbd, last)] {
                let physical = 0x8000 + (address & 0xfff);
                cases.push(
                    Case::replacing_flags(
                        format!("{name}, opcode {opcode:x}, source {source_bytes:02x?}"),
                        &[prefix, &[0x0f, opcode], address_bytes].concat(),
                        flags,
                    )
                    .initial_registers(registers)
                    .expect_register(destination, Exact(result))
                    .map_page(address >> 12, 0x8000, ReadOnly)
                    .backing(physical - 1, &[0x5a])
                    .backing(physical, &source_bytes)
                    .backing(physical + source_bytes.len() as u32, &[0x5a]),
                );
            }
        }
    }
    cases
}
test_cases!(old_destination_supplies_the_address, address_alias_cases());

fn fault_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, address, present, fault) in [
        (&[0x66][..], 0x4000, false, 0x4000),
        (&[][..], 0x4000, false, 0x4000),
        (&[0x66][..], 0x4fff, true, 0x5000),
        (&[][..], 0x4ffd, true, 0x5000),
        (&[][..], 0x4ffe, true, 0x5000),
        (&[][..], 0x4fff, true, 0x5000),
        (&[0x66][..], 0xffff_ffff, true, 0xffff_ffff),
        (&[][..], 0xffff_fffe, true, 0xffff_fffe),
    ] {
        for opcode in [0xbc, 0xbd] {
            // A set bit in the first byte cannot suppress the remaining read checks.
            for first_byte in [0, 1] {
                let mut case = Case::preserving_flags(format!("opcode {opcode:x}, prefix {prefix:02x?}, source at {address:x}, first byte {first_byte}"),
                    &[prefix, &[0x0f, opcode, 0x03]].concat())
                    .initial_register(Gpr32::Eax, 0x4433_a55b).initial_register(Gpr32::Ebx, address)
                    .backing(0x8000 + (address & 0xfff), &[first_byte]).fault(fault, 0);
                if present {
                    case = case.map_page(address >> 12, 0x8000, ReadOnly);
                }
                cases.push(case);
            }
        }
    }
    cases
}
test_cases!(
    source_faults_precede_destination_and_flag_effects,
    fault_cases()
);
