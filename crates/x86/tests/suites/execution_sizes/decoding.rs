use super::code16;
use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};
use wasm86_x86::Gpr32::{Eax, Ebx, Esi};

fn defaults_and_prefixes() -> Vec<Case> {
    let mut cases = vec![
        code16(
            Case::preserving_flags("CS.D=0 selects a word immediate", &[0xb8, 0x78, 0x56])
                .register(Eax, 0xaaaa_0000, 0xaaaa_5678),
        ),
        code16(
            Case::preserving_flags(
                "66 selects a dword immediate in 16-bit code",
                &[0x66, 0xb8, 0x78, 0x56, 0x34, 0x12],
            )
            .register(Eax, 0, 0x1234_5678),
        ),
        code16(
            Case::preserving_flags("CS.D does not truncate straight-line EIP", &[0x90])
                .at(0xffff)
                .dispatch(0x10000),
        ),
        code16(
            Case::preserving_flags("32-bit near target in 16-bit code", &[0x66, 0xff, 0xe0])
                .initial_register(Eax, 0x12345)
                .dispatch(0x12345),
        ),
        code16(
            Case::preserving_flags("16-bit near target truncates EIP", &[0xff, 0xe0])
                .initial_register(Eax, 0x12345)
                .dispatch(0x2345),
        ),
    ];
    for prefixes in [&[0x66, 0x67][..], &[0x67, 0x66], &[0x66, 0x67, 0x66, 0x67]] {
        let code = [prefixes, &[0xa1, 0, 0x40]].concat();
        cases.push(
            Case::preserving_flags(
                format!("independent prefix presence {prefixes:02x?}"),
                &code,
            )
            .register(Eax, 0xaaaa_0000, 0xaaaa_5678)
            .memory(0x4000, &[0x78, 0x56], ReadOnly),
        );
        let code = [prefixes, &[0xa1, 0, 0x40, 1, 0]].concat();
        cases.push(code16(
            Case::preserving_flags(format!("inverted defaults {prefixes:02x?}"), &code)
                .register(Eax, 0, 0x1234_5678)
                .memory(0x14000, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        ));
    }
    for code in [
        &[0x67, 0x8b, 0x06, 0, 0x40][..],
        &[0x67, 0x8b, 0x80, 0, 0x40],
        &[0x67, 0xa1, 0, 0x40],
    ] {
        cases.push(
            Case::preserving_flags(format!("exact disp16 at page end {code:02x?}"), code)
                .at(0x2000 - code.len() as u32)
                .initial_registers(&[(Ebx, 0), (Esi, 0)])
                .register(Eax, 0, 0x1234_5678)
                .memory(0x4000, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        );
    }
    cases
}

test_cases!(
    code_defaults_prefix_presence_and_exact_fetches,
    defaults_and_prefixes()
);
