//! Native loaded values and full-width register values stay on their exact paths.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use relocade_x86::{Compiler, CpuState};
use wasmparser::{Operator, Parser, Payload};

#[test]
fn loaded_values_use_native_rounding() {
    let code = [0xdd, 0x05, 0, 0x40, 0, 0, 0xd9, 0xfc]; // FLD m64; FRNDINT
    for rc in 0..4 {
        let mut observed = CpuState::default();
        set_control(&mut observed.x87.control, 0x007f | (rc << 10));
        let compiled = Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&observed)
            .compile_block(0x1000, &code, 2)
            .unwrap();
        let operations = Parser::new(0)
            .parse_all(&compiled.bytes)
            .filter_map(|payload| match payload.unwrap() {
                Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
                _ => None,
            })
            .flat_map(|reader| reader.into_iter().map(Result::unwrap))
            .filter_map(|op| match op {
                Operator::F64Nearest => Some(0),
                Operator::F64Floor => Some(1),
                Operator::F64Ceil => Some(2),
                Operator::F64Trunc => Some(3),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(operations, [rc]);
        let module = TestModule::new(&compiled);
        for (input, negative, inexact, lower, upper, nearest_up) in [
            (1.5_f64, false, true, (LEAD, 0x3fff), (LEAD, 0x4000), true),
            (-1.5, true, true, (LEAD, 0xbfff), (LEAD, 0xc000), true),
            (-0.25, true, true, (0, 0x8000), (LEAD, 0xbfff), false),
            (-0.0, true, false, (0, 0x8000), (0, 0x8000), false),
            (
                f64::MAX,
                false,
                false,
                (0xffff_ffff_ffff_f800, 0x43fe),
                (0xffff_ffff_ffff_f800, 0x43fe),
                false,
            ),
        ] {
            let mut image = stack_image(&code, 0, 0xffff);
            image.cpu.x87.control = observed.x87.control;
            image.cpu.x87.status.precision = 0;
            image.map(4, 0x8000, false);
            image.data(0x8000, &input.to_le_bytes());
            let up = inexact
                && match rc {
                    0 => nearest_up,
                    1 => negative,
                    2 => !negative,
                    _ => false,
                };
            let mut result = complete_x87(image.cpu, 6, 0x0505);
            result = complete_x87(result, 2, 0x01fc);
            result.x87.status.top = 7;
            result.x87.status.precision = u8::from(inexact);
            result.x87.status.c1 = u8::from(up);
            result.x87.data_offset = 0x4000;
            result.x87.data_selector = 0x23;
            write_value(&mut result, 7, if up { upper } else { lower });
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(&image, &[dispatch(result)]),
                "native RC={rc} input={input}"
            );
        }
    }
}

#[test]
fn full_precision_and_rejected_operands() {
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &CODE, 1)
        .unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (input, output, invalid, denormal, precision) in [
        ((LEAD, 0x7fff), (LEAD, 0x7fff), 0, 0, 0),
        ((LEAD + 1, 0xffff), (0xc000_0000_0000_0001, 0xffff), 1, 0, 0),
        ((1, 0x8000), (0, 0x8000), 0, 1, 1),
    ] {
        let mut image = stack_image(&CODE, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, input);
        image.check_unchanged_exit(&block, "FRNDINT restarts before effects", Exit::Interpret);
        let mut result = complete_x87(image.cpu, 2, 0x01fc);
        result.x87.status.c1 = 0;
        result.x87.status.invalid = invalid;
        result.x87.status.denormal = denormal;
        result.x87.status.precision = precision;
        write_value(&mut result, 0, output);
        assert_eq!(
            linked.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
    // Without CPU observations, all rounding modes stay compiled at full precision.
    for rc in 0..4 {
        let mut image = stack_image(&CODE, 0, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x007f | (rc << 10));
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, (LEAD + 1, 0x3ffe));
        let up = rc == 0 || rc == 2;
        let mut result = complete_x87(image.cpu, 2, 0x01fc);
        result.x87.status.precision = 1;
        result.x87.status.c1 = u8::from(up);
        write_value(&mut result, 0, if up { (LEAD, 0x3fff) } else { (0, 0) });
        assert_eq!(
            block.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}
