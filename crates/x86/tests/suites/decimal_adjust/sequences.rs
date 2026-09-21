use super::*;
use crate::support::sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence};
use wasm86_x86::Gpr32::Ebx;

fn unpacked_arithmetic() -> Vec<Sequence> {
    let addition = Flags {
        af: Set,
        pf: Set,
        ..Flags::all(Clear)
    };
    vec![
        Sequence::new(
            "AAA reads pending ADD flags and supplies ADC carry",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x4433_1209), (Ebx, 0)])
        .step(Step::new(&[0x04, 9], addition).register(Eax, 0x4433_1212))
        .step(Step::new(&[0x37], unpacked_flags(true)).register(Eax, 0x4433_1308))
        .step(Step::new(&[0x80, 0xd3, 0], Flags::all(Clear)).register(Ebx, 1)),
        Sequence::new(
            "AAS reads pending SUB flags and supplies SETB",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x4433_1200), (Ebx, 0)])
        .step(
            Step::new(
                &[0x2c, 9],
                Flags {
                    cf: Set,
                    af: Set,
                    sf: Set,
                    ..Flags::all(Clear)
                },
            )
            .register(Eax, 0x4433_12f7),
        )
        .step(Step::new(&[0x3f], unpacked_flags(true)).register(Eax, 0x4433_1101))
        .step(Step::preserving_flags(&[0x0f, 0x92, 0xc3]).register(Ebx, 1)),
        Sequence::new(
            "AAM zero base publishes earlier adjustments and stops",
            Flags::all(false),
        )
        .initial_register(Eax, 0x4433_1209)
        .instruction_count(u32::MAX - 1)
        .step(Step::new(&[0x04, 9], addition).register(Eax, 0x4433_1212))
        .step(Step::new(&[0x37], unpacked_flags(true)).register(Eax, 0x4433_1308))
        .step(Step::preserving_flags(&[0xd4, 0]).divide_error())
        .trailing_code(&[0xd5, 10], 1),
    ]
}

fn packed_arithmetic() -> Vec<Sequence> {
    vec![
        Sequence::new(
            "DAA reads pending ADD flags and supplies LAHF",
            Flags::all(false),
        )
        .initial_register(Eax, 0x4433_2279)
        .step(
            Step::new(
                &[0x04, 0x35],
                Flags {
                    sf: Set,
                    of: Set,
                    ..Flags::all(Clear)
                },
            )
            .register(Eax, 0x4433_22ae),
        )
        .step(Step::new(&[0x27], packed_flags(0x14, true, true)).register(Eax, 0x4433_2214))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_1714)),
        Sequence::new(
            "DAS reads pending SUB flags and supplies LAHF",
            Flags::all(false),
        )
        .initial_register(Eax, 0x4433_2235)
        .step(
            Step::new(
                &[0x2c, 0x47],
                Flags {
                    cf: Set,
                    pf: Set,
                    af: Set,
                    sf: Set,
                    ..Flags::all(Clear)
                },
            )
            .register(Eax, 0x4433_22ee),
        )
        .step(Step::new(&[0x2f], packed_flags(0x88, true, true)).register(Eax, 0x4433_2288))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9788)),
    ]
}

fn radix_aliases() -> Vec<Sequence> {
    vec![Sequence::new(
        "radix adjustments follow EAX, AH, AL and AX writes",
        Flags::all(true),
    )
    .initial_registers(&[(Eax, 0), (Ebx, 0)])
    .step(Step::preserving_flags(&[0xb8, 0xfe, 0xab, 0x34, 0x12]).register(Eax, 0x1234_abfe))
    .step(Step::new(&[0xd4, 255], digit_flags(0xfe)).register(Eax, 0x1234_00fe))
    .step(Step::preserving_flags(&[0x0f, 0x98, 0xc3]).register(Ebx, 1))
    .step(Step::preserving_flags(&[0xb4, 9]).register(Eax, 0x1234_09fe))
    .step(Step::preserving_flags(&[0xb0, 9]).register(Eax, 0x1234_0909))
    .step(Step::new(&[0xd5, 10], digit_flags(0x63)).register(Eax, 0x1234_0063))
    .step(Step::preserving_flags(&[0x66, 0xb8, 0x51, 0xff]).register(Eax, 0x1234_ff51))
    .step(Step::new(&[0xd4, 10], digit_flags(1)).register(Eax, 0x1234_0801))]
}

test_sequences!(unpacked_flags_and_divide_error, unpacked_arithmetic());
test_sequences!(
    packed_flags_feed_following_instructions,
    packed_arithmetic()
);
test_sequences!(
    radix_adjustments_use_current_register_aliases,
    radix_aliases()
);
