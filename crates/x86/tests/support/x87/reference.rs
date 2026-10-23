//! Literal reference bounds shared by x87 instruction suites.

pub(crate) mod trigonometric;

pub(crate) struct Bounds {
    pub(crate) lower: (u64, u16),
    pub(crate) upper: (u64, u16),
    pub(crate) nearest_up: bool,
}

impl Bounds {
    pub(crate) fn rounded(&self, rc: u16, flip_sign: bool) -> ((u64, u16), bool) {
        let negative = (self.lower.1 & 0x8000 != 0) ^ flip_sign;
        let increment = self.lower != self.upper
            && match rc {
                0 => self.nearest_up,
                1 => negative,
                2 => !negative,
                _ => false,
            };
        let output = if increment { self.upper } else { self.lower };
        (
            (output.0, output.1 ^ if flip_sign { 0x8000 } else { 0 }),
            increment,
        )
    }
}
