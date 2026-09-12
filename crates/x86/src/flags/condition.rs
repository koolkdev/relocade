use wasm86_compiler::{MemoryInt, Val, I1};

use super::{FlagMask, StatusFlag};

pub(crate) type OperandComparison<T> = fn(&Val<T>, &Val<T>) -> Val<I1>;
pub(crate) type ResultComparison<T> = fn(&Val<T>) -> Val<I1>;

/// The discriminant is the low four opcode bits; its low bit inverts a pair.
#[repr(u8)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Condition {
    O,
    NO,
    B,
    AE,
    E,
    NE,
    BE,
    A,
    S,
    NS,
    P,
    NP,
    L,
    GE,
    LE,
    G,
}

impl Condition {
    pub(crate) fn flags(self) -> FlagMask {
        use StatusFlag::*;
        match self.canonical() {
            Self::O => FlagMask::of(OF),
            Self::B => FlagMask::of(CF),
            Self::E => FlagMask::of(ZF),
            Self::BE => FlagMask::of(CF).union(FlagMask::of(ZF)),
            Self::S => FlagMask::of(SF),
            Self::P => FlagMask::of(PF),
            Self::L => FlagMask::of(SF).union(FlagMask::of(OF)),
            Self::LE => FlagMask::of(ZF)
                .union(FlagMask::of(SF))
                .union(FlagMask::of(OF)),
            _ => unreachable!("only canonical conditions have distinct flag dependencies"),
        }
    }

    /// One predicate per inverse pair, in opcode order. Stored-condition readers
    /// can share that predicate and invert its result for the paired condition.
    pub(crate) const CANONICAL: [Self; 8] = [
        Self::O,
        Self::B,
        Self::E,
        Self::BE,
        Self::S,
        Self::P,
        Self::L,
        Self::LE,
    ];

    pub(crate) const fn canonical(self) -> Self {
        Self::CANONICAL[(self as usize) >> 1]
    }

    pub(crate) const fn is_inverted(self) -> bool {
        self as u8 & 1 != 0
    }

    pub(crate) const fn from_code(code: u8) -> Self {
        match code & 15 {
            0 => Self::O,
            1 => Self::NO,
            2 => Self::B,
            3 => Self::AE,
            4 => Self::E,
            5 => Self::NE,
            6 => Self::BE,
            7 => Self::A,
            8 => Self::S,
            9 => Self::NS,
            10 => Self::P,
            11 => Self::NP,
            12 => Self::L,
            13 => Self::GE,
            14 => Self::LE,
            15 => Self::G,
            _ => unreachable!(),
        }
    }

    /// Returns the relation implied by subtracting right from left. Its absence
    /// means this condition needs result flags. Selecting a relation authors no
    /// expressions; applying it preserves the operands' logical width.
    pub(crate) fn operand_comparison<T: MemoryInt>(self) -> Option<OperandComparison<T>> {
        Some(match self {
            Self::B => |left, right| left.unsigned().lt(right),
            Self::AE => |left, right| left.unsigned().ge(right),
            Self::E => |left, right| left.eq(right),
            Self::NE => |left, right| left.ne(right),
            Self::BE => |left, right| right.unsigned().ge(left),
            Self::A => |left, right| right.unsigned().lt(left),
            Self::L => |left, right| left.signed().lt(right),
            Self::GE => |left, right| left.signed().ge(right),
            Self::LE => |left, right| right.signed().ge(left),
            Self::G => |left, right| right.signed().lt(left),
            _ => return None,
        })
    }

    /// Returns the zero/nonzero test that a logical result answers directly.
    /// The unary predicate needs no unused record operand or flag image.
    pub(crate) fn logic_result_comparison<T: MemoryInt>(self) -> Option<ResultComparison<T>> {
        Some(match self {
            Self::E => |result| result.eq(0),
            Self::NE => |result| result.ne(0),
            _ => return None,
        })
    }

    /// Reads only the status bits needed by this condition. The fallible callback
    /// lets CPU-backed reads and pure arithmetic sources share the predicate rules.
    pub(crate) fn evaluate<E>(
        self,
        mut read: impl FnMut(StatusFlag) -> Result<Val<I1>, E>,
    ) -> Result<Val<I1>, E> {
        use StatusFlag::*;
        Ok(match self {
            Self::O => read(OF)?,
            Self::NO => read(OF)?.eq(0),
            Self::B => read(CF)?,
            Self::AE => read(CF)?.eq(0),
            Self::E => read(ZF)?,
            Self::NE => read(ZF)?.eq(0),
            Self::BE => read(CF)?.or(read(ZF)?),
            Self::A => read(CF)?.or(read(ZF)?).eq(0),
            Self::S => read(SF)?,
            Self::NS => read(SF)?.eq(0),
            Self::P => read(PF)?,
            Self::NP => read(PF)?.eq(0),
            Self::L => read(SF)?.xor(read(OF)?),
            Self::GE => read(SF)?.xor(read(OF)?).eq(0),
            Self::LE => read(ZF)?.or(read(SF)?.xor(read(OF)?)),
            Self::G => read(ZF)?.or(read(SF)?.xor(read(OF)?)).eq(0),
        })
    }
}
