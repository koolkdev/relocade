//! Exact integer sources participate in native PC53 arithmetic and precise restart.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use relocade_x86::Compiler;
use wasmparser::{Operator, Parser, Payload};

#[test]
fn integer_sources_reach_native_operations() {
    for source in [Source::Integer16(-3), Source::Integer32(-3)] {
        for (extension, result, operation) in [
            (0, (0xc000_0000_0000_0000, 0x4000), "add"),
            (1, (0x9000_0000_0000_0000, 0xc003), "multiply"),
            (4, (0x9000_0000_0000_0000, 0x4002), "add"),
            (5, (0x9000_0000_0000_0000, 0xc002), "add"),
            (6, (LEADING, 0xc000), "divide"),
            (7, (LEADING, 0xbffe), "divide"),
        ] {
            let code = [
                vec![0xdd, 0x05, 8, 0x40, 0, 0],
                source.instruction(extension, 0x4000),
            ]
            .concat();
            let mut image = stack_image(&code, 0, 0xffff);
            image.cpu.x87.status.precision = 0;
            set_control(&mut image.cpu.x87.control, 0x027f);
            image.map(4, 0x8000, false);
            image.data(0x8000, &source.bytes());
            image.data(0x8008, &6.0_f64.to_le_bytes());
            let compiled = Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&image.cpu)
                .compile_block(0x1000, &code, 2)
                .unwrap();
            let native = Parser::new(0).parse_all(&compiled.bytes).any(|payload| {
                let Payload::CodeSectionEntry(body) = payload.unwrap() else {
                    return false;
                };
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .any(|op| match op.unwrap() {
                        Operator::F64Add => operation == "add",
                        Operator::F64Mul => operation == "multiply",
                        Operator::F64Div => operation == "divide",
                        _ => false,
                    })
            });
            assert!(native, "integer source must reach native {operation}");
            let mut loaded = complete_x87(image.cpu, 6, 0x0505);
            loaded.x87.status.top = 7;
            loaded.x87.status.c1 = 0;
            loaded.x87.data_offset = 0x4008;
            loaded.x87.data_selector = 0x23;
            write_value(&mut loaded, 7, (0xc000_0000_0000_0000, 0x4001));
            let mut cpu = completed_memory(loaded, source, extension);
            write_value(&mut cpu, 7, result);
            assert_eq!(
                TestModule::new(&compiled).observe(&image.input(), 1),
                expected(&image, &[dispatch(cpu)]),
                "native extension {extension}, {source:?}"
            );
        }
    }
}

#[test]
fn operand_exception_restarts_after_the_completed_load() {
    let source = Source::Integer32(0);
    let code = [
        vec![0xdd, 0x05, 8, 0x40, 0, 0],
        source.instruction(6, 0x4000),
    ]
    .concat();
    for control in [0x027f, 0x027b] {
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        set_control(&mut image.cpu.x87.control, control);
        image.map(4, 0x8000, false);
        image.data(0x8000, &source.bytes());
        image.data(0x8008, &1.0_f64.to_le_bytes());
        let compiled = Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&image.cpu)
            .compile_block(0x1000, &code, 2)
            .unwrap();
        let mut loaded = complete_x87(image.cpu, 6, 0x0505);
        loaded.x87.status.top = 7;
        loaded.x87.status.c1 = 0;
        loaded.x87.data_offset = 0x4008;
        loaded.x87.data_selector = 0x23;
        write_value(&mut loaded, 7, (LEADING, 0x3fff));
        assert_eq!(
            TestModule::new(&compiled).observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: loaded,
                    ram: &[],
                    exit: Exit::Interpret
                }]
            )
        );
        let mut result = completed_memory(loaded, source, 6);
        result.x87.status.zero_divide = 1;
        if control & 4 != 0 {
            write_value(&mut result, 7, (LEADING, 0x7fff));
        } else {
            result.x87.status.error_summary = 1;
            result.x87.status.busy = 1;
        }
        let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
        assert_eq!(
            linked.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}
