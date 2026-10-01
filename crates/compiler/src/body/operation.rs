//! Operations keep their attributes separate from ordered value inputs.

use crate::{
    memory::{AtomicKind, AtomicUpdate, Location, MemoryAccess},
    Func,
};

#[derive(Clone)]
pub(crate) struct Operation {
    kind: OperationKind,
    inputs: Inputs,
}

#[derive(Clone, Copy)]
pub(crate) enum OperationKind {
    Load {
        access: MemoryAccess,
    },
    Store {
        access: MemoryAccess,
    },
    Call {
        target: Func,
    },
    Atomic {
        access: MemoryAccess,
        operator: AtomicKind,
    },
    Fence,
}

impl Operation {
    pub(crate) fn load(access: MemoryAccess, address: usize) -> Self {
        Self::new(OperationKind::Load { access }, [address])
    }

    pub(crate) fn store(access: MemoryAccess, address: usize, value: usize) -> Self {
        Self::new(OperationKind::Store { access }, [address, value])
    }

    pub(crate) fn call(target: Func, arguments: Vec<usize>) -> Self {
        Self::new(OperationKind::Call { target }, arguments)
    }

    pub(crate) fn atomic_load(access: MemoryAccess, address: usize) -> Self {
        Self::new(
            OperationKind::Atomic {
                access,
                operator: AtomicKind::Load,
            },
            [address],
        )
    }

    pub(crate) fn atomic_store(access: MemoryAccess, address: usize, value: usize) -> Self {
        Self::new(
            OperationKind::Atomic {
                access,
                operator: AtomicKind::Store,
            },
            [address, value],
        )
    }

    pub(crate) fn atomic_compare_exchange(
        access: MemoryAccess,
        address: usize,
        expected: usize,
        replacement: usize,
    ) -> Self {
        Self::new(
            OperationKind::Atomic {
                access,
                operator: AtomicKind::CompareExchange,
            },
            [address, expected, replacement],
        )
    }

    pub(crate) fn atomic_update(
        access: MemoryAccess,
        operator: AtomicUpdate,
        address: usize,
        value: usize,
    ) -> Self {
        Self::new(
            OperationKind::Atomic {
                access,
                operator: AtomicKind::Update(operator),
            },
            [address, value],
        )
    }

    pub(crate) fn fence() -> Self {
        Self::new(OperationKind::Fence, [])
    }

    pub(crate) fn kind(&self) -> OperationKind {
        self.kind
    }

    /// Inputs follow Wasm stack order, with the address first for memory accesses.
    fn new(kind: OperationKind, inputs: impl Into<Inputs>) -> Self {
        Self {
            kind,
            inputs: inputs.into(),
        }
    }

    pub(crate) fn inputs(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        self.inputs.as_slice().iter().copied()
    }

    pub(crate) fn map_inputs(mut self, mut map: impl FnMut(usize) -> usize) -> Self {
        for input in self.inputs.as_mut_slice() {
            *input = map(*input);
        }
        self
    }

    pub(crate) fn location(&self) -> Option<Location> {
        match self.kind {
            OperationKind::Load { access }
            | OperationKind::Store { access }
            | OperationKind::Atomic { access, .. } => Some(access.at(self.inputs.as_slice()[0])),
            OperationKind::Call { .. } | OperationKind::Fence => None,
        }
    }
}

/// Fixed inputs stay inline; calls with more inputs retain their argument buffer.
#[derive(Clone)]
enum Inputs {
    Empty,
    One([usize; 1]),
    Two([usize; 2]),
    Three([usize; 3]),
    Many(Vec<usize>),
}

impl Inputs {
    fn as_slice(&self) -> &[usize] {
        match self {
            Self::Empty => &[],
            Self::One(values) => values,
            Self::Two(values) => values,
            Self::Three(values) => values,
            Self::Many(values) => values,
        }
    }

    fn as_mut_slice(&mut self) -> &mut [usize] {
        match self {
            Self::Empty => &mut [],
            Self::One(values) => values,
            Self::Two(values) => values,
            Self::Three(values) => values,
            Self::Many(values) => values,
        }
    }
}

impl<const N: usize> From<[usize; N]> for Inputs {
    fn from(values: [usize; N]) -> Self {
        match values.as_slice() {
            [] => Self::Empty,
            &[one] => Self::One([one]),
            &[one, two] => Self::Two([one, two]),
            &[one, two, three] => Self::Three([one, two, three]),
            _ => Self::Many(values.into()),
        }
    }
}

impl From<Vec<usize>> for Inputs {
    fn from(values: Vec<usize>) -> Self {
        match values.as_slice() {
            [] => Self::Empty,
            &[one] => Self::One([one]),
            &[one, two] => Self::Two([one, two]),
            &[one, two, three] => Self::Three([one, two, three]),
            _ => Self::Many(values),
        }
    }
}
