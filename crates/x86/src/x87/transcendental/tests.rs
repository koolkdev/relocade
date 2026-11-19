//! Boundaries of the shared series termination rule.

use super::*;
use crate::{test_step as step, CompiledModule};
use relocade_compiler::Program;

#[test]
fn convergence_requires_absorption_and_bounded_next_terms() {
    let mut program = Program::new();
    let function = program.function::<fn(u64, u64, u64) -> bool>(|body, (low, term, factor)| {
        let sum = DoubleDouble::from_parts((1.0.into(), Val::<F64>::from_bits(low)));
        let term = DoubleDouble::from_f64(Val::<F64>::from_bits(term));
        let factor = DoubleDouble::from_f64(Val::<F64>::from_bits(factor));
        body.return_(sum.absorbs_series_tail(&term, &factor))
    });
    program.export("converged", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "converged".into(),
        execution_target: None,
    });

    let low = 2_f64.powi(-60);
    let small = 2_f64.powi(-116);
    for (low, term, factor, expected) in [
        (low, small, 0.75, true),
        (-low, -small, -0.75, true),
        (low, -low, 0.5, false),
        (low, small, -1.0, false),
        (2_f64.powi(-446), 2_f64.powi(-503), 0.5, false),
        (0.0, small, 0.5, false),
        (0.0, -0.0, 0.5, true),
        (-0.0, 0.0, 0.5, false),
        (0.0, 0.0, f64::INFINITY, false),
    ] {
        let input = step::Input {
            arguments: [low, term, factor]
                .map(|value| step::Argument::I64(value.to_bits() as i64))
                .into(),
            ..step::Input::new(&[])
        };
        assert_eq!(
            module.observe(&input, 1).events,
            [step::Event::Return {
                outcome: step::Outcome::Returned(vec![step::Argument::I32(i32::from(expected))]),
                snapshot: step::Snapshot {
                    cpu: vec![],
                    guest: None,
                },
            }],
            "sum.low={low}, term={term}, factor={factor}"
        );
    }
}
