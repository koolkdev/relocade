use wasm86_x86::Gpr32;

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    InstructionCase,
};

use super::{bit_flags, other_register_inputs, Operation, INITIAL_FLAGS, OPERATIONS, STORED_FLAGS};

fn literal_results() -> Vec<InstructionCase> {
    [
        (Operation::Bt, 0x8000_0001, 31, 0x8000_0001, Set),
        (Operation::Bt, 0x8000_0001, 30, 0x8000_0001, Clear),
        (Operation::Bts, 1, 0, 1, Set),
        (Operation::Bts, 0, 31, 0x8000_0000, Clear),
        (Operation::Btr, 0, 0, 0, Clear),
        (Operation::Btr, u32::MAX, 31, 0x7fff_ffff, Set),
        (Operation::Btc, 0, 0, 1, Clear),
        (Operation::Btc, 1, 32, 0, Set),
    ]
    .into_iter()
    .map(|(operation, input, index, output, carry)| {
        InstructionCase::new(
            format!("{operation:?} {input:x}, index {index} publishes the old bit"),
            &[0x0f, 0xba, 0xc0 | (operation.extension() << 3), index],
            INITIAL_FLAGS,
            bit_flags(carry),
        )
        .initial_registers(&other_register_inputs(&[Gpr32::Eax]))
        .stored_flags(STORED_FLAGS)
        .register(Gpr32::Eax, input, output)
    })
    .collect()
}

test_cases!(old_bit_and_unchanged_writes, literal_results());

fn aliased_indexes() -> Vec<InstructionCase> {
    struct Alias {
        bits: u32,
        register: Gpr32,
        modrm: u8,
        input: u32,
        // BT, BTS, BTR, BTC, in that order.
        outputs: [u32; 4],
        carry: crate::support::cases::FlagExpectation,
    }
    let mut cases = Vec::new();
    for alias in [
        Alias {
            bits: 16,
            register: Gpr32::Eax,
            modrm: 0xc0,
            input: 0x4433_8003,
            outputs: [0x4433_8003, 0x4433_800b, 0x4433_8003, 0x4433_800b],
            carry: Clear,
        },
        Alias {
            bits: 16,
            register: Gpr32::Ecx,
            modrm: 0xc9,
            input: 0x8877_ffff,
            outputs: [0x8877_ffff, 0x8877_ffff, 0x8877_7fff, 0x8877_7fff],
            carry: Set,
        },
        Alias {
            bits: 16,
            register: Gpr32::Esp,
            modrm: 0xe4,
            input: 0x8765_0010,
            outputs: [0x8765_0010, 0x8765_0011, 0x8765_0010, 0x8765_0011],
            carry: Clear,
        },
        Alias {
            bits: 32,
            register: Gpr32::Eax,
            modrm: 0xc0,
            input: 0x4433_8003,
            outputs: [0x4433_8003, 0x4433_800b, 0x4433_8003, 0x4433_800b],
            carry: Clear,
        },
        Alias {
            bits: 32,
            register: Gpr32::Ecx,
            modrm: 0xc9,
            input: 0x8877_ffff,
            outputs: [0x8877_ffff, 0x8877_ffff, 0x0877_ffff, 0x0877_ffff],
            carry: Set,
        },
        Alias {
            bits: 32,
            register: Gpr32::Esp,
            modrm: 0xe4,
            input: 0x8765_0010,
            outputs: [0x8765_0010, 0x8765_0010, 0x8764_0010, 0x8764_0010],
            carry: Set,
        },
    ] {
        for (operation, output) in OPERATIONS.into_iter().zip(alias.outputs) {
            let mut code = if alias.bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, operation.register_opcode(), alias.modrm]);
            cases.push(
                InstructionCase::new(
                    format!(
                        "{operation:?} {}-bit destination and index share {:?}",
                        alias.bits, alias.register
                    ),
                    &code,
                    INITIAL_FLAGS,
                    bit_flags(alias.carry),
                )
                .initial_registers(&other_register_inputs(&[alias.register]))
                .stored_flags(STORED_FLAGS)
                .register(alias.register, alias.input, output),
            );
        }
    }
    cases
}

test_cases!(indexes_capture_the_old_destination, aliased_indexes());
