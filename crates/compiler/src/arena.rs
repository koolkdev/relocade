//! Shared graph construction, lexical visibility and unbound-expression admission.
use crate::{
    body::{
        BlockId, BlockItem, Effect, EffectId, Exit, FunctionGraph, Layout, Operation, Value,
        ValueDefinition, ValueTable,
    },
    integer::BitBounds,
    value::UnboundExpression,
    BuildError, Expression, Type,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone)]
pub(super) struct FunctionArena(Rc<RefCell<Option<Construction>>>);
struct Construction {
    graph: FunctionGraph,
    unbound: HashMap<UnboundExpression, usize>,
    scopes: Vec<usize>,
}

impl FunctionArena {
    pub(super) fn new() -> Self {
        Self(Rc::new(RefCell::new(Some(Construction {
            graph: FunctionGraph::new(),
            unbound: HashMap::new(),
            scopes: vec![0],
        }))))
    }
    pub(super) fn same_body(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub(super) fn check_open(&self) -> Result<(), BuildError> {
        if self.0.borrow().is_some() {
            Ok(())
        } else {
            Err(BuildError::BodyClosed)
        }
    }
    pub(super) fn constant(&self, ty: Type, bits: u64) -> Result<usize, BuildError> {
        self.with_open(|table| table.constant(ty, bits))
    }
    pub(crate) fn resolve_unbound(
        &self,
        expression: &UnboundExpression,
    ) -> Result<usize, BuildError> {
        {
            let arena = self.0.borrow();
            let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
            if let Some(&value) = arena.unbound.get(expression) {
                return Ok(value);
            }
        }
        let value = expression.build(self)?;
        self.0
            .borrow_mut()
            .as_mut()
            .ok_or(BuildError::BodyClosed)?
            .unbound
            .insert(expression.clone(), value);
        Ok(value)
    }
    pub(super) fn parameter(&self, ty: Type, component: usize) -> Result<usize, BuildError> {
        self.with_graph(|graph| {
            if let Some(&value) = graph.blocks[0].parameters.get(component) {
                return value;
            }
            // Parameters are initialized in signature order before consumers build values.
            assert_eq!(graph.blocks[0].parameters.len(), component);
            let unsigned = ty.bits();
            let carrier = if ty == Type::I64 { 64 } else { 32 };
            let value = graph.values.push_with_bounds(
                Value {
                    ty,
                    definition: ValueDefinition::Parameter {
                        block: graph.entry,
                        component,
                    },
                },
                BitBounds {
                    unsigned,
                    signed: unsigned.saturating_add(1).min(carrier),
                },
            );
            graph.blocks[0].parameters.push(value);
            value
        })
    }
    pub(super) fn block(&self, scope: usize, types: &[Type]) -> Result<BlockId, BuildError> {
        self.with_graph(|graph| graph.block(scope, types))
    }
    pub(super) fn parameters(&self, block: BlockId) -> Result<Vec<usize>, BuildError> {
        self.with_graph(|graph| graph.blocks[block.0].parameters.clone())
    }
    pub(super) fn exit(&self, block: BlockId, exit: Exit) -> Result<(), BuildError> {
        self.with_graph(|graph| graph.blocks[block.0].exit = exit)
    }
    pub(super) fn execute(
        &self,
        block: BlockId,
        operation: Operation,
        types: &[Type],
    ) -> Result<Vec<usize>, BuildError> {
        self.with_graph(|graph| {
            let effect = EffectId(graph.effects.len());
            let results: Vec<_> = types
                .iter()
                .enumerate()
                .map(|(component, &ty)| {
                    graph.values.push(Value {
                        ty,
                        definition: ValueDefinition::Result {
                            producer: BlockItem::Effect(effect),
                            component,
                        },
                    })
                })
                .collect();
            graph.effects.push(Effect {
                results: results.clone(),
                operation,
                origin: block,
            });
            graph.blocks[block.0].items.push(BlockItem::Effect(effect));
            results
        })
    }
    pub(super) fn complete_join(
        &self,
        target: BlockId,
        sources: &[BlockId],
    ) -> Result<Vec<usize>, BuildError> {
        self.with_graph(|graph| {
            let mut incoming = Vec::new();
            let mut seen = vec![false; graph.blocks.len()];
            let mut pending = sources.to_vec();
            while let Some(block) = pending.pop() {
                if block == target || std::mem::replace(&mut seen[block.0], true) {
                    continue;
                }
                for edge in graph.blocks[block.0].exit.edges() {
                    if edge.target == target {
                        incoming.push(edge.arguments.clone());
                    } else if graph.blocks[edge.target.0].scope != graph.blocks[target.0].scope {
                        pending.push(edge.target);
                    }
                }
            }
            if incoming.is_empty() && !graph.blocks[target.0].parameters.is_empty() {
                return Err(BuildError::MissingBranchValue);
            }
            for (component, &parameter) in graph.blocks[target.0].parameters.iter().enumerate() {
                if let Some(bounds) = incoming
                    .iter()
                    .map(|values| graph.values.bounds[values[component]])
                    .reduce(BitBounds::union)
                {
                    graph.values.bounds[parameter] = bounds;
                }
            }
            Ok(graph.blocks[target.0].parameters.clone())
        })?
    }
    pub(super) fn child_scope(&self, parent: usize) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        let arena = arena.as_mut().ok_or(BuildError::BodyClosed)?;
        let scope = arena.scopes.len();
        arena.scopes.push(parent);
        Ok(scope)
    }
    pub(super) fn definition_scope(&self, value: usize) -> Result<usize, BuildError> {
        self.with_graph(|graph| match graph.values[value].definition {
            ValueDefinition::Parameter { block, .. } => graph.blocks[block.0].scope,
            ValueDefinition::Result {
                producer: BlockItem::Effect(effect),
                ..
            } => graph.blocks[graph.effects[effect.0].origin.0].scope,
            ValueDefinition::Constant(_)
            | ValueDefinition::Expression(_)
            | ValueDefinition::Result {
                producer: BlockItem::Evaluate(_),
                ..
            } => {
                unreachable!("calculated handles retain their original operand scopes")
            }
        })
    }
    pub(super) fn merge_scopes(
        &self,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<Option<usize>, BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        Ok(arena.merge_scopes(left, right))
    }
    pub(super) fn require_visible(
        &self,
        required: Option<usize>,
        scope: usize,
    ) -> Result<(), BuildError> {
        let arena = self.0.borrow();
        let arena = arena.as_ref().ok_or(BuildError::BodyClosed)?;
        if required.is_some_and(|owner| arena.contains(owner, scope)) {
            Ok(())
        } else {
            Err(BuildError::OutOfScope)
        }
    }
    pub(super) fn require_scope(&self, owner: usize, scope: usize) -> Result<(), BuildError> {
        self.require_visible(Some(owner), scope)
    }
    pub(super) fn expression(
        &self,
        ty: Type,
        expression: Expression<usize>,
        component: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|table| crate::expression::build(table, ty, expression, component))
    }
    pub(super) fn normalize(&self, input: usize) -> Result<usize, BuildError> {
        self.with_open(|table| crate::expression::normalize(table, input))
    }
    fn with_open(&self, build: impl FnOnce(&mut ValueTable) -> usize) -> Result<usize, BuildError> {
        self.with_graph(|graph| build(&mut graph.values))
    }
    fn with_graph<R>(&self, build: impl FnOnce(&mut FunctionGraph) -> R) -> Result<R, BuildError> {
        let mut arena = self.0.borrow_mut();
        Ok(build(
            &mut arena.as_mut().ok_or(BuildError::BodyClosed)?.graph,
        ))
    }
    pub(super) fn finish(&self, layout: Vec<Layout>) -> Result<FunctionGraph, BuildError> {
        let mut graph = self
            .0
            .borrow_mut()
            .take()
            .ok_or(BuildError::BodyClosed)?
            .graph;
        graph.layout = layout;
        Ok(graph)
    }
    pub(super) fn close(&self) {
        self.0.borrow_mut().take();
    }
    #[cfg(test)]
    pub(super) fn take(&self) -> Option<ValueTable> {
        self.0.borrow_mut().take().map(|arena| arena.graph.values)
    }
}
impl Construction {
    fn contains(&self, owner: usize, mut scope: usize) -> bool {
        while scope != owner && scope != 0 {
            scope = self.scopes[scope];
        }
        scope == owner
    }
    fn merge_scopes(&self, left: Option<usize>, right: Option<usize>) -> Option<usize> {
        let left = left?;
        let right = right?;
        if self.contains(left, right) {
            Some(right)
        } else if self.contains(right, left) {
            Some(left)
        } else {
            None
        }
    }
}
