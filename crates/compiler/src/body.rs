//! Typed values, effects and explicit control edges owned by one function.
use crate::{memory::Mem, Expression, Func, Type};

mod operation;
mod producer;
mod values;
pub(super) use operation::{Operation, OperationKind};
pub(super) use producer::{BlockItem, Effect, EffectId};
pub(super) use values::ValueTable;

pub(super) struct FunctionGraph {
    pub(super) values: ValueTable,
    pub(super) effects: Vec<Effect>,
    pub(super) blocks: Vec<Block>,
    pub(super) layout: Vec<Layout>,
    pub(super) entry: BlockId,
    pub(super) memories: Vec<Mem>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct BlockId(pub(super) usize);

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Value {
    pub(super) ty: Type,
    pub(super) definition: ValueDefinition,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum ValueDefinition {
    Constant(u64),
    Expression(Expression<usize>),
    Parameter {
        block: BlockId,
        component: usize,
    },
    Result {
        producer: BlockItem,
        component: usize,
    },
}

pub(super) struct Block {
    pub(super) parameters: Vec<usize>,
    pub(super) items: Vec<BlockItem>,
    pub(super) exit: Exit,
    pub(super) scope: usize,
}

#[derive(Clone)]
pub(super) struct Edge {
    pub(super) target: BlockId,
    pub(super) arguments: Vec<usize>,
}

#[derive(Clone, Default)]
pub(super) enum Exit {
    #[default]
    Open,
    Jump(Edge),
    If {
        condition: usize,
        taken: Edge,
        otherwise: Edge,
    },
    Switch {
        selector: usize,
        cases: Vec<(u32, Edge)>,
        default: Edge,
    },
    Return(Vec<usize>),
    TailCall {
        target: Func,
        arguments: Vec<usize>,
    },
    Trap,
}

/// Wasm nesting references the graph's blocks; it owns no effects or operands.
pub(super) enum Layout {
    Block(BlockId),
    Scope {
        preheader: BlockId,
        body: Vec<Layout>,
        after: BlockId,
    },
    If {
        branch: BlockId,
        taken: Vec<Layout>,
        otherwise: Vec<Layout>,
        join: BlockId,
    },
    Loop {
        preheader: BlockId,
        header: BlockId,
        body: Vec<Layout>,
        after: BlockId,
    },
    Switch {
        branch: BlockId,
        cases: Vec<(u32, Vec<Layout>)>,
        default: Vec<Layout>,
        join: BlockId,
    },
}

impl FunctionGraph {
    pub(super) fn new() -> Self {
        Self {
            values: ValueTable::default(),
            effects: Vec::new(),
            blocks: vec![Block {
                parameters: Vec::new(),
                items: Vec::new(),
                exit: Exit::Open,
                scope: 0,
            }],
            layout: Vec::new(),
            entry: BlockId(0),
            memories: Vec::new(),
        }
    }

    pub(super) fn block(&mut self, scope: usize, types: &[Type]) -> BlockId {
        let block = BlockId(self.blocks.len());
        let parameters = types
            .iter()
            .enumerate()
            .map(|(component, &ty)| {
                self.values.push(Value {
                    ty,
                    definition: ValueDefinition::Parameter { block, component },
                })
            })
            .collect();
        self.blocks.push(Block {
            parameters,
            items: Vec::new(),
            exit: Exit::Open,
            scope,
        });
        block
    }

    pub(super) fn outgoing(&self, block: BlockId) -> Vec<&Edge> {
        match &self.blocks[block.0].exit {
            Exit::If {
                condition,
                taken,
                otherwise,
            } => match self.values[*condition].definition {
                ValueDefinition::Constant(0) => vec![otherwise],
                ValueDefinition::Constant(_) => vec![taken],
                _ => vec![taken, otherwise],
            },
            Exit::Switch {
                selector,
                cases,
                default,
            } => match self.values[*selector].definition {
                ValueDefinition::Constant(bits) => vec![cases
                    .iter()
                    .find(|(key, _)| u64::from(*key) == bits)
                    .map_or(default, |(_, edge)| edge)],
                _ => self.blocks[block.0].exit.edges(),
            },
            _ => self.blocks[block.0].exit.edges(),
        }
    }

    pub(super) fn reachable(&self) -> Vec<bool> {
        let mut seen = vec![false; self.blocks.len()];
        let mut pending = vec![self.entry];
        while let Some(block) = pending.pop() {
            if std::mem::replace(&mut seen[block.0], true) {
                continue;
            }
            pending.extend(self.outgoing(block).into_iter().map(|edge| edge.target));
        }
        seen
    }
}

impl Exit {
    pub(super) fn edges_mut(&mut self) -> Vec<&mut Edge> {
        match self {
            Self::Jump(edge) => vec![edge],
            Self::If {
                taken, otherwise, ..
            } => vec![taken, otherwise],
            Self::Switch { cases, default, .. } => cases
                .iter_mut()
                .map(|(_, edge)| edge)
                .chain(std::iter::once(default))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn edges(&self) -> Vec<&Edge> {
        match self {
            Self::Jump(edge) => vec![edge],
            Self::If {
                taken, otherwise, ..
            } => vec![taken, otherwise],
            Self::Switch { cases, default, .. } => cases
                .iter()
                .map(|(_, edge)| edge)
                .chain(std::iter::once(default))
                .collect(),
            _ => Vec::new(),
        }
    }
    pub(super) fn inputs(&self) -> Vec<usize> {
        match self {
            Self::Return(arguments) | Self::TailCall { arguments, .. } => arguments.clone(),
            Self::If { condition, .. } => std::iter::once(*condition)
                .chain(
                    self.edges()
                        .into_iter()
                        .flat_map(|edge| edge.arguments.iter().copied()),
                )
                .collect(),
            Self::Switch { selector, .. } => std::iter::once(*selector)
                .chain(
                    self.edges()
                        .into_iter()
                        .flat_map(|edge| edge.arguments.iter().copied()),
                )
                .collect(),
            _ => self
                .edges()
                .into_iter()
                .flat_map(|edge| edge.arguments.iter().copied())
                .collect(),
        }
    }
    pub(super) fn map_inputs(&mut self, mut map: impl FnMut(usize) -> usize) {
        let mut edge = |edge: &mut Edge| {
            for input in &mut edge.arguments {
                *input = map(*input);
            }
        };
        match self {
            Self::Jump(value) => edge(value),
            Self::If {
                condition,
                taken,
                otherwise,
            } => {
                edge(taken);
                edge(otherwise);
                *condition = map(*condition);
            }
            Self::Switch {
                selector,
                cases,
                default,
            } => {
                for (_, value) in cases {
                    edge(value);
                }
                edge(default);
                *selector = map(*selector);
            }
            Self::Return(arguments) | Self::TailCall { arguments, .. } => {
                for input in arguments {
                    *input = map(*input);
                }
            }
            Self::Open | Self::Trap => {}
        }
    }
}
