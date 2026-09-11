use wasm86_x86::Gpr32::Ebx;

use super::{test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case};

fn origin_and_exit_cases() -> Vec<Case> {
    let initial = Flags {
        cf: true,
        pf: false,
        af: true,
        zf: false,
        sf: true,
        of: false,
    };
    let expected = Flags {
        cf: Preserved,
        pf: Preserved,
        af: Preserved,
        zf: Preserved,
        sf: Preserved,
        of: Preserved,
    };
    vec![
        Case::new("origin before dispatch", &[0xeb, 0x03], initial, expected)
            .at(0x1ffe)
            .dispatch(0x2003),
        Case::new("dispatch before origin", &[0xeb, 0x03], initial, expected)
            .dispatch(0x2003)
            .at(0x1ffe),
        Case::new("origin before fault", &[0x89, 0x03], initial, expected)
            .at(0x3000)
            .fault(0x6000, 2)
            .initial_register(Ebx, 0x6000),
        Case::new("fault before origin", &[0x89, 0x03], initial, expected)
            .fault(0x6000, 2)
            .at(0x3000)
            .initial_register(Ebx, 0x6000),
    ]
}

test_cases!(
    origin_and_exit_expectations_compose_in_either_order,
    origin_and_exit_cases()
);
