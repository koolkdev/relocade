use super::*;

struct Example {
    code: [u8; 2],
    ax: u16,
    result: u16,
}

#[rustfmt::skip]
fn conversions() -> Vec<Case> {
    let examples = [
        Example { code: [0xd4, 10], ax: 0xab00, result: 0x0000 },
        Example { code: [0xd4, 10], ax: 0xab09, result: 0x0009 },
        Example { code: [0xd4, 10], ax: 0xab0a, result: 0x0100 },
        Example { code: [0xd4, 10], ax: 0xab51, result: 0x0801 },
        Example { code: [0xd4, 10], ax: 0xabff, result: 0x1905 },
        Example { code: [0xd4, 1], ax: 0xabff, result: 0xff00 },
        Example { code: [0xd4, 2], ax: 0xabff, result: 0x7f01 },
        Example { code: [0xd4, 8], ax: 0xabff, result: 0x1f07 },
        Example { code: [0xd4, 16], ax: 0xabff, result: 0x0f0f },
        Example { code: [0xd4, 128], ax: 0xabff, result: 0x017f },
        Example { code: [0xd4, 255], ax: 0xabfe, result: 0x00fe },
        Example { code: [0xd4, 255], ax: 0xabff, result: 0x0100 },
        Example { code: [0xd5, 10], ax: 0x0909, result: 0x0063 },
        Example { code: [0xd5, 10], ax: 0xffff, result: 0x00f5 },
        Example { code: [0xd5, 10], ax: 0x1906, result: 0x0000 },
        Example { code: [0xd5, 0], ax: 0xab80, result: 0x0080 },
        Example { code: [0xd5, 1], ax: 0x01ff, result: 0x0000 },
        Example { code: [0xd5, 2], ax: 0x8080, result: 0x0080 },
        Example { code: [0xd5, 8], ax: 0x1f07, result: 0x00ff },
        Example { code: [0xd5, 16], ax: 0x0f0f, result: 0x00ff },
        Example { code: [0xd5, 128], ax: 0x0201, result: 0x0001 },
        Example { code: [0xd5, 255], ax: 0x0101, result: 0x0000 },
        Example { code: [0xd5, 255], ax: 0xffff, result: 0x0000 },
    ];
    examples.into_iter().map(|example| Case::new(
        format!("{:02x?} converts AX={:04x}", example.code, example.ax),
        &example.code, Flags::all(true), digit_flags(example.result as u8),
    ).register(Eax, 0x4433_0000 | u32::from(example.ax), 0x4433_0000 | u32::from(example.result))).collect()
}

fn zero_base() -> Vec<Case> {
    [0, 0x4433_ab51, u32::MAX]
        .into_iter()
        .map(|eax| {
            Case::preserving_flags(format!("AAM zero base with EAX={eax:08x}"), &[0xd4, 0])
                .initial_register(Eax, eax)
                .divide_error()
        })
        .collect()
}

test_cases!(encoded_bases_and_byte_wrapping, conversions());
test_cases!(aam_zero_base_preserves_the_fault_boundary, zero_base());
