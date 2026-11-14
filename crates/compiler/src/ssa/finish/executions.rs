//! Most semantic values have one execution; allocate a history only for copies.
use crate::body::BlockId;

#[derive(Default)]
pub(super) enum Executions {
    #[default]
    None,
    One((BlockId, usize)),
    Many(Vec<(BlockId, usize)>),
}

impl Executions {
    pub(super) fn as_slice(&self) -> &[(BlockId, usize)] {
        match self {
            Self::None => &[],
            Self::One(execution) => std::slice::from_ref(execution),
            Self::Many(executions) => executions,
        }
    }

    pub(super) fn push(&mut self, execution: (BlockId, usize)) {
        match self {
            Self::None => *self = Self::One(execution),
            Self::One(previous) => *self = Self::Many(vec![*previous, execution]),
            Self::Many(executions) => executions.push(execution),
        }
    }
}
