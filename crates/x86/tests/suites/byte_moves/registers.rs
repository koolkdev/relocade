use crate::support::cases::{test_cases, InstructionCase as Case};
use wasm86_x86::Gpr32;

// Rows name the byte views explicitly; outputs are complete parent-register values.
struct ByteRegister {
    name: &'static str,
    parent: Gpr32,
    input: u32,
    immediate: u8,
    after_immediate: u32,
    // Source order: AL, CL, DL, BL, AH, CH, DH, BH.
    after_moves: [u32; 8],
}

#[rustfmt::skip]
const REGISTERS: [ByteRegister; 8] = [
    ByteRegister { name: "AL", parent: Gpr32::Eax, input: 0x4433_2211, immediate: 0x80, after_immediate: 0x4433_2280, after_moves: [0x4433_2211, 0x4433_2255, 0x4433_2299, 0x4433_22dd, 0x4433_2222, 0x4433_2266, 0x4433_22aa, 0x4433_22ee] },
    ByteRegister { name: "CL", parent: Gpr32::Ecx, input: 0x8877_6655, immediate: 0x00, after_immediate: 0x8877_6600, after_moves: [0x8877_6611, 0x8877_6655, 0x8877_6699, 0x8877_66dd, 0x8877_6622, 0x8877_6666, 0x8877_66aa, 0x8877_66ee] },
    ByteRegister { name: "DL", parent: Gpr32::Edx, input: 0xccbb_aa99, immediate: 0xff, after_immediate: 0xccbb_aaff, after_moves: [0xccbb_aa11, 0xccbb_aa55, 0xccbb_aa99, 0xccbb_aadd, 0xccbb_aa22, 0xccbb_aa66, 0xccbb_aaaa, 0xccbb_aaee] },
    ByteRegister { name: "BL", parent: Gpr32::Ebx, input: 0x10ff_eedd, immediate: 0x66, after_immediate: 0x10ff_ee66, after_moves: [0x10ff_ee11, 0x10ff_ee55, 0x10ff_ee99, 0x10ff_eedd, 0x10ff_ee22, 0x10ff_ee66, 0x10ff_eeaa, 0x10ff_eeee] },
    ByteRegister { name: "AH", parent: Gpr32::Eax, input: 0x4433_2211, immediate: 0x88, after_immediate: 0x4433_8811, after_moves: [0x4433_1111, 0x4433_5511, 0x4433_9911, 0x4433_dd11, 0x4433_2211, 0x4433_6611, 0x4433_aa11, 0x4433_ee11] },
    ByteRegister { name: "CH", parent: Gpr32::Ecx, input: 0x8877_6655, immediate: 0x8a, after_immediate: 0x8877_8a55, after_moves: [0x8877_1155, 0x8877_5555, 0x8877_9955, 0x8877_dd55, 0x8877_2255, 0x8877_6655, 0x8877_aa55, 0x8877_ee55] },
    ByteRegister { name: "DH", parent: Gpr32::Edx, input: 0xccbb_aa99, immediate: 0xb7, after_immediate: 0xccbb_b799, after_moves: [0xccbb_1199, 0xccbb_5599, 0xccbb_9999, 0xccbb_dd99, 0xccbb_2299, 0xccbb_6699, 0xccbb_aa99, 0xccbb_ee99] },
    ByteRegister { name: "BH", parent: Gpr32::Ebx, input: 0x10ff_eedd, immediate: 0x7f, after_immediate: 0x10ff_7fdd, after_moves: [0x10ff_11dd, 0x10ff_55dd, 0x10ff_99dd, 0x10ff_dddd, 0x10ff_22dd, 0x10ff_66dd, 0x10ff_aadd, 0x10ff_eedd] },
 ];

#[rustfmt::skip]
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (destination_code, destination) in REGISTERS.iter().enumerate() {
        cases.push(Case::preserving_flags(format!("MOV {},imm8", destination.name), &[0xb0 + destination_code as u8, destination.immediate])
            .register(destination.parent, destination.input, destination.after_immediate));
        for (source_code, source) in REGISTERS.iter().enumerate() {
            for code in [
                [0x88, 0xc0 | ((source_code as u8) << 3) | destination_code as u8],
                [0x8a, 0xc0 | ((destination_code as u8) << 3) | source_code as u8],
            ] {
                let mut case = Case::preserving_flags(format!("MOV {},{} via {:02x}", destination.name, source.name, code[0]), &code)
                    .register(destination.parent, destination.input, destination.after_moves[source_code]);
                if source.parent != destination.parent {
                    case = case.initial_register(source.parent, source.input);
                }
                cases.push(case);
            }
        }
    }
    cases.push(Case::preserving_flags("MOV AH,imm8: complete immediate at mapped page end", &[0xb4, 0x88])
        .register(Gpr32::Eax, 0x4433_2211, 0x4433_8811).at(0x1ffe));
    cases.push(Case::preserving_flags("MOV BH,imm8: fetch wraps EIP", &[0xb7, 0x8a])
        .register(Gpr32::Ebx, 0x10ff_eedd, 0x10ff_8add).at(0xffff_ffff)
        .map_page(0xfffff, 0x8000, crate::support::cases::Permissions::ReadOnly)
        .map_page(0, 0xa000, crate::support::cases::Permissions::ReadOnly));
    cases
}

test_cases!(all_byte_register_encodings, cases());
