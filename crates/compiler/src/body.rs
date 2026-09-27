//! Completed function bodies and their value and control relationships.
//!
//! Construction creates these records; compiler passes inspect or transform them.
//! Builder state and Wasm encoding belong to their respective modules.

use crate::{
    memory::{AtomicOperation, Location},
    Expression, Func, Type,
};

mod tree;
pub(super) use tree::BlockTree;
mod values;
pub(super) use values::ValueTable;

pub(super) struct Body {
    pub(super) values: Vec<Value>,
    pub(super) block: Block,
}

pub(super) enum Terminal {
    Trap,
    Branch {
        target: Target,
        arguments: Vec<usize>,
    },
    Return(Vec<usize>),
    TailCall(Invocation),
}

impl Terminal {
    pub(super) fn inputs(&self) -> &[usize] {
        match self {
            Self::Trap => &[],
            Self::Return(arguments) | Self::Branch { arguments, .. } => arguments,
            Self::TailCall(invocation) => &invocation.arguments,
        }
    }
}

pub(super) enum Operation {
    // Keep authored sites stable when control folding removes an operation.
    Nop,
    Load {
        location: Location,
    },
    Store {
        location: Location,
        value: usize,
    },
    Atomic {
        access: AtomicOperation,
        output: Option<usize>,
    },
    Fence,
    Block {
        block: Block,
        outputs: Vec<usize>,
    },
    Loop {
        initial: Vec<usize>,
        inputs: Vec<usize>,
        block: Block,
        outputs: Vec<usize>,
    },
    If {
        condition: usize,
        branch: Block,
        else_branch: Option<Block>,
        outputs: Vec<usize>,
    },
    BranchIf {
        condition: usize,
        taken: Block,
    },
    Switch {
        selector: usize,
        cases: Vec<SwitchCase>,
        default: Block,
        outputs: Vec<usize>,
    },
    Call {
        invocation: Invocation,
        outputs: Vec<usize>,
    },
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Value {
    pub(super) ty: Type,
    pub(super) definition: ValueDefinition,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum ValueDefinition {
    Constant(u64),
    Parameter(u32),
    LoopInput { block: usize, component: usize },
    Expression(Expression<usize>),
    Load { site: Site },
    OperationResult { site: Site, component: usize },
    JoinResult { site: Site, component: usize },
}

pub(super) struct SwitchCase {
    pub(super) key: u32,
    pub(super) block: Block,
}

impl Operation {
    pub(super) fn children(&self) -> impl DoubleEndedIterator<Item = &Block> {
        let (first, second, cases): (_, _, &[SwitchCase]) = match self {
            Self::Block { block, .. } | Self::Loop { block, .. } => (Some(block), None, &[]),
            Self::BranchIf { taken, .. } => (Some(taken), None, &[]),
            Self::If {
                branch,
                else_branch,
                ..
            } => (Some(branch), else_branch.as_ref(), &[]),
            Self::Switch { cases, default, .. } => (Some(default), None, cases),
            _ => (None, None, &[]),
        };
        cases
            .iter()
            .map(|case| &case.block)
            .chain(first)
            .chain(second)
    }

    pub(super) fn branch_outputs(&self) -> &[usize] {
        match self {
            Self::Block { outputs, .. }
            | Self::Loop { outputs, .. }
            | Self::If { outputs, .. }
            | Self::Switch { outputs, .. } => outputs,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Site {
    pub(super) block: usize,
    pub(super) index: usize,
}

/// A loop's header and result join occupy the same authored control site.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Target {
    pub(super) site: Site,
    pub(super) entry: bool,
}

impl Target {
    pub(super) fn exit(site: Site) -> Self {
        Self { site, entry: false }
    }

    pub(super) fn entry(site: Site) -> Self {
        Self { site, entry: true }
    }
}

pub(super) struct Block {
    pub(super) id: usize,
    pub(super) operations: Vec<Operation>,
    pub(super) terminal: Option<Terminal>,
}

impl Block {
    pub(super) fn walk(&self) -> Blocks<'_> {
        Blocks(vec![self])
    }

    pub(super) fn exits_to(&self, target: Target) -> impl Iterator<Item = (Site, &[usize])> {
        self.walk().filter_map(move |block| match &block.terminal {
            Some(Terminal::Branch {
                target: destination,
                arguments,
            }) if *destination == target => Some((
                Site {
                    block: block.id,
                    index: block.operations.len(),
                },
                arguments.as_slice(),
            )),
            _ => None,
        })
    }
}

pub(super) struct Blocks<'a>(Vec<&'a Block>);

impl<'a> Iterator for Blocks<'a> {
    type Item = &'a Block;
    fn next(&mut self) -> Option<Self::Item> {
        let block = self.0.pop()?;
        for operation in block.operations.iter().rev() {
            self.0.extend(operation.children().rev());
        }
        Some(block)
    }
}

pub(super) struct Invocation {
    pub(super) target: Func,
    pub(super) arguments: Vec<usize>,
}
