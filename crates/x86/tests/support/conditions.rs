use super::{
    cases::Flags,
    guest::Permissions::ReadWrite,
    sequences::{Checkpoint, SequenceCase},
};

pub(crate) struct ConditionExample {
    pub(crate) name: &'static str,
    pub(crate) flags: Flags<bool>,
    /// Literal outcomes in O, NO, B, AE, E, NE, BE, A, S, NS, P, NP, L, GE, LE, G order.
    pub(crate) results: [bool; 16],
}

// These inputs distinguish each condition, including signed/unsigned disagreement
// and the independent terms of compound predicates. Families sample encoding widths.
#[rustfmt::skip]
pub(crate) const CONDITION_EXAMPLES: [ConditionExample; 6] = [
    ConditionExample {
        name: "condition inputs clear",
        flags: Flags { cf: false, pf: false, af: true, zf: false, sf: false, of: false },
        results: [false, true, false, true, false, true, false, true, false, true, false, true, false, true, false, true],
    },
    ConditionExample {
        name: "equal without carry",
        flags: Flags { cf: false, pf: true, af: true, zf: true, sf: false, of: false },
        results: [false, true, false, true, true, false, true, false, false, true, true, false, false, true, true, false],
    },
    ConditionExample {
        name: "carry and overflow",
        flags: Flags { cf: true, pf: true, af: true, zf: true, sf: false, of: true },
        results: [true, false, true, false, true, false, true, false, false, true, true, false, true, false, true, false],
    },
    ConditionExample {
        name: "negative without overflow",
        flags: Flags { cf: false, pf: false, af: true, zf: false, sf: true, of: false },
        results: [false, true, false, true, false, true, false, true, true, false, false, true, true, false, true, false],
    },
    ConditionExample {
        name: "signed and unsigned disagreement",
        flags: Flags { cf: true, pf: true, af: false, zf: false, sf: true, of: true },
        results: [true, false, true, false, false, true, true, false, true, false, true, false, false, true, false, true],
    },
    ConditionExample {
        name: "overflow without carry",
        flags: Flags { cf: false, pf: true, af: true, zf: false, sf: true, of: true },
        results: [true, false, false, true, false, true, false, true, true, false, true, false, false, true, false, true],
    },
];

impl SequenceCase {
    /// Append all sixteen condition stores using EDI=0x6000 and guarded output
    /// backing. The caller supplies literal results in O, NO, B, AE, ... LE, G order.
    pub(crate) fn conditions(mut self, results: [u8; 16]) -> Self {
        self = self
            .initial_register(wasm86_x86::Gpr32::Edi, 0x6000)
            .map_page(6, 0xa000, ReadWrite)
            .backing(0x9fff, &[0xa5; 18]);
        for (condition, result) in results.into_iter().enumerate() {
            assert!(result <= 1, "a condition expectation is a literal bit");
            let condition = condition as u8;
            self = self.step(
                Checkpoint::preserving_flags(&[
                    0x0f,
                    0x90 + condition,
                    0x47 | ((condition & 7) << 3),
                    condition,
                ])
                .expect_memory(0x6000 + u32::from(condition), &[result]),
            );
        }
        self
    }
}
