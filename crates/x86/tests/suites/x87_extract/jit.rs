//! Both extracted values retain native views; rejected cases restart before the push.

use super::*;
use crate::support::wasm::operators;
use crate::support::{machine::expected, step::TestModule};
use relocade_x86::Compiler;
use wasmparser::Operator;

fn native_values() {
    let code = [0xdd, 0x05, 0, 0x40, 0, 0, 0xd9, 0xf4, 0xd8, 0xc1];
    for (input, sign) in [(3.0_f64, 0), (-0.75, 0x8000)] {
        let mut image = stack_image(&code, 0, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x027f);
        image.cpu.x87.status.precision = 0;
        image.map(4, 0x8000, false);
        image.data(0x8000, &input.to_le_bytes());
        let compiled = Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&image.cpu)
            .compile_block(0x1000, &code, 3)
            .unwrap();
        let native_add = operators(&compiled.bytes).any(|op| matches!(op, Operator::F64Add));
        assert!(native_add);
        let mut result = complete_x87(image.cpu, 6, 0x0505);
        result = complete_x87(result, 2, 0x01f4);
        result = complete_x87(result, 2, 0x00c1);
        result.x87.status.top = 6;
        result.x87.status.c1 = 0;
        result.x87.data_offset = 0x4000;
        result.x87.data_selector = 0x23;
        write_value(&mut result, 7, (LEAD, 0x3fff | sign));
        write_value(&mut result, 6, (0xa000_0000_0000_0000, 0x4000 | sign)); // ±1.5 plus ±1
        assert_eq!(
            TestModule::new(&compiled).observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

fn restart() {
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &CODE, 1)
        .unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (input, sig, exp, flag) in [
        ((0, 0x8000), (0, 0x8000), (LEAD, 0xffff), 4),
        ((1, 0), (LEAD, 0x3fff), (0x807a_0000_0000_0000, 0xc00d), 2),
        (
            (LEAD + 1, 0xffff),
            (0xc000_0000_0000_0001, 0xffff),
            (0xc000_0000_0000_0001, 0xffff),
            1,
        ),
    ] {
        let mut image = stack_image(&CODE, 0, 0xffff);
        write_value(&mut image.cpu, 0, input);
        image.check_unchanged_exit(
            &block,
            "FXTRACT restarts before either slot or TOP changes",
            Exit::Interpret,
        );
        let mut result = complete_x87(image.cpu, 2, 0x01f4);
        result.x87.status.c1 = 0;
        result.x87.status.top = 7;
        result.x87.status.invalid = u8::from(flag == 1);
        result.x87.status.denormal = u8::from(flag == 2);
        result.x87.status.zero_divide = u8::from(flag == 4);
        write_value(&mut result, 0, exp);
        write_value(&mut result, 7, sig);
        assert_eq!(
            linked.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

#[test]
fn native_consumers_and_precise_restart() {
    native_values();
    restart();
}
