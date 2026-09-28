use super::*;
use wasm86_compiler::{I16, I64};

#[test]
fn masked_bit_facts_preserve_the_unobserved_high_bits() {
    for expected_low in [0_u32, 3] {
        let module = Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
            let input = body.parameter::<I32>(0)?;
            body.if_(input.and(7).eq(expected_low), |arm| arm.return_(&input))?;
            body.return_(99)
        });
        for input in [0_i32, 3, 8, 11, 256, 259, i32::MIN, i32::MIN + 3, -1] {
            assert_eq!(
                module.instantiate().call::<i32>(input),
                Ok(if input & 7 == expected_low as i32 {
                    input
                } else {
                    99
                })
            );
        }
    }
}

#[test]
fn a_truncated_bit_fact_preserves_the_remaining_byte() {
    let module = Fixture::new().function(&[Type::I8], &[Type::I32], |mut body| {
        let byte = body.parameter::<I8>(0)?;
        body.if_(byte.truncate::<I1>(), |arm| arm.return_(7))?;
        body.return_(byte.unsigned().extend::<I32>().add(256))
    });
    for input in 0..256 {
        assert_eq!(
            module.instantiate().call::<i32>((input,)),
            Ok(if input & 1 != 0 { 7 } else { input + 256 })
        );
    }
}

#[test]
fn a_proved_negative_byte_keeps_its_signed_carrier_at_a_join() {
    let module = Fixture::new().function(&[Type::I8], &[Type::I32, Type::I64], |mut body| {
        let byte = body
            .parameter::<I8>(0)?
            .signed()
            .extend::<I32>()
            .truncate::<I8>();
        let result = body.if_value::<I8>(
            byte.eq(255),
            |arm| arm.yield_(&byte),
            |arm| arm.yield_(&byte),
        )?;
        body.return_((
            result.signed().extend::<I32>(),
            result.signed().extend::<I64>(),
        ))
    });
    for input in 0..256 {
        let expected = i32::from(input as u8 as i8);
        assert_eq!(
            module.instantiate().call::<(i32, i64)>((input,)),
            Ok((expected, i64::from(expected)))
        );
    }
}

#[test]
fn signed_shift_and_division_retain_their_logical_widths() {
    let module = Fixture::new().function(
        &[Type::I16, Type::I1],
        &[Type::I16, Type::I16],
        |mut body| {
            let word = body.parameter::<I16>(0)?;
            let flag = body.parameter::<I1>(1)?;
            body.if_(&flag, |arm| arm.return_((0_u32, 0_u32)))?;
            let shift = flag.select(0, 1);
            body.return_((word.signed().shr(shift), word.signed().div(3)))
        },
    );
    for input in [0, 1, 32767, 32768, 65534, 65535] {
        let signed = i32::from(input as u16 as i16);
        assert_eq!(
            module.instantiate().call::<(i32, i32)>((input, 0)),
            Ok(((signed >> 1) & 65535, (signed / 3) & 65535))
        );
    }
}
