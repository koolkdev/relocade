//! Store selection observes exact low bytes without changing shared conversions.
use super::*;

fn conversions(v8: bool) {
    let initial = [0xa5; 32];
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I64], &[Type::I32; 2], |mut body| {
        let wide = body.parameter::<I64>(0)?;
        body.store(memory, 1, wide.truncate::<I8>())?;
        body.store(memory, 3, wide.truncate::<I16>())?;
        let shared = wide.truncate::<I32>();
        body.store(memory, 7, &shared)?;
        body.store(memory, 12, &shared)?;
        let byte = wide.truncate::<I8>();
        let signed = byte.signed().extend::<I32>();
        body.store(memory, 17, signed.truncate::<I8>())?;
        // This store observes the sign-filled byte too: extension must remain.
        body.store(memory, 19, signed.truncate::<I16>())?;
        let unsigned = byte.unsigned().extend::<I32>();
        body.store(memory, 23, unsigned.truncate::<I8>())?;
        body.return_((shared, unsigned))
    });
    let mut stores = Vec::new();
    for payload in Parser::new(0).parse_all(module.bytes()) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                match op.unwrap() {
                    Operator::I64Store8 { memarg } => stores.push((1, memarg.offset)),
                    Operator::I64Store16 { memarg } => stores.push((2, memarg.offset)),
                    Operator::I64Store32 { memarg } => stores.push((4, memarg.offset)),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(stores, [(1, 1), (2, 3), (4, 7), (4, 12), (1, 17), (1, 23)]);
    for (input, low, word, signed_word) in [
        (0x1234_5678_90ab_cd80_i64, 0x80, 0xcd80_u16, 0xff80_u16),
        (0x7654_3210_1234_567f_i64, 0x7f, 0x567f_u16, 0x007f_u16),
    ] {
        let mut expected = initial;
        expected[1] = low;
        expected[3..5].copy_from_slice(&word.to_le_bytes());
        expected[7..11].copy_from_slice(&(input as u32).to_le_bytes());
        expected[12..16].copy_from_slice(&(input as u32).to_le_bytes());
        expected[17] = low;
        expected[19..21].copy_from_slice(&signed_word.to_le_bytes());
        expected[23] = low;
        let returned = [Value::I32(input as i32), Value::I32(i32::from(low))];
        if v8 {
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &[Value::I64(input)])
                        .with_memories(&[MemoryBytes::new("state", &initial)])
                ),
                Observation::returned(&returned)
                    .with_memories(&[MemoryBytes::new("state", &expected)])
            );
        } else {
            let mut instance = module.instantiate();
            assert_eq!(
                instance.call_values("run", &[Value::I64(input)]).unwrap(),
                returned
            );
            assert_eq!(&instance.memory("state")[..32], expected);
        }
    }
}

#[test]
fn scalar_stores_absorb_only_unobserved_conversion_bits() {
    conversions(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_scalar_stores_absorb_only_unobserved_conversion_bits() {
    conversions(true);
}
