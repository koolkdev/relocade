//! Structured regions, result joins and branch construction.
use crate::{results, Arguments, BuildError, FunctionBuilder, Operation, Results, Terminal, Type};

mod block;
mod conditional;
mod switch;

pub use block::Label;

pub(super) struct SwitchCase {
    pub(super) key: u32,
    pub(super) region: Region,
}

impl Operation {
    pub(super) fn children(&self) -> impl DoubleEndedIterator<Item = &Region> {
        let (first, second, cases): (_, _, &[SwitchCase]) = match self {
            Self::Block { region, .. } => (Some(region), None, &[]),
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
            .map(|case| &case.region)
            .chain(first)
            .chain(second)
    }

    pub(super) fn branch_outputs(&self) -> &[usize] {
        match self {
            Self::Block { outputs, .. }
            | Self::If { outputs, .. }
            | Self::Switch { outputs, .. } => outputs,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Site {
    pub(super) region: usize,
    pub(super) index: usize,
}

pub(super) struct Region {
    pub(super) id: usize,
    pub(super) operations: Vec<Operation>,
    pub(super) terminal: Option<Terminal>,
}

impl Region {
    pub(super) fn new(id: usize) -> Self {
        Self {
            id,
            operations: Vec::new(),
            terminal: None,
        }
    }

    pub(super) fn walk(&self) -> Regions<'_> {
        Regions(vec![self])
    }

    pub(super) fn exits_to(&self, target: Site) -> impl Iterator<Item = (Site, &[usize])> {
        self.walk()
            .filter_map(move |region| match &region.terminal {
                Some(Terminal::Branch {
                    target: destination,
                    arguments,
                }) if *destination == target => Some((
                    Site {
                        region: region.id,
                        index: region.operations.len(),
                    },
                    arguments.as_slice(),
                )),
                _ => None,
            })
    }
}

pub(super) struct Regions<'a>(Vec<&'a Region>);

impl<'a> Iterator for Regions<'a> {
    type Item = &'a Region;
    fn next(&mut self) -> Option<Self::Item> {
        let region = self.0.pop()?;
        for operation in region.operations.iter().rev() {
            self.0.extend(operation.children().rev());
        }
        Some(region)
    }
}

#[derive(Clone)]
pub(super) struct JoinTarget {
    site: Site,
    pub(super) types: Vec<Type>,
}

pub(super) enum Destination<'a> {
    Function,
    Branch {
        region: &'a mut Option<Region>,
        target: Option<JoinTarget>,
    },
}

impl FunctionBuilder<'_> {
    /// Supplies the enclosing result arm or block's values, consuming its builder.
    /// The declared result shape determines native literals' logical types; typed
    /// values must match it. Scalar arguments stay scalar, and tuples follow the
    /// corresponding tuple of result types. A unit result accepts `()`.
    ///
    /// Execution continues after this control operation. Only a direct result
    /// arm or block may yield; use [`Self::branch`] to leave an enclosing block
    /// from a nested branch.
    pub fn yield_(mut self, arguments: impl Into<Arguments>) -> Result<(), BuildError> {
        self.fallthrough = false;
        let Destination::Branch {
            target: Some(target),
            ..
        } = &self.destination
        else {
            return Err(BuildError::InvalidYield);
        };
        let target = target.clone();
        let arguments = self.result_arguments(arguments, &target.types)?;
        self.complete(Terminal::Branch {
            target: target.site,
            arguments,
        })
    }

    fn result_target<R: Results>(&self) -> JoinTarget {
        JoinTarget {
            site: self.site(),
            types: results::types::<R>(),
        }
    }

    fn build_branch(
        &mut self,
        target: Option<&JoinTarget>,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Region, BuildError> {
        let scope = self.arena.child_scope(self.region.id)?;
        self.build_region(scope, target, build)
    }

    fn build_region(
        &mut self,
        scope: usize,
        target: Option<&JoinTarget>,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Region, BuildError> {
        let mut destination = None;
        build(FunctionBuilder {
            program: self.program,
            function: self.function,
            arena: self.arena.clone(),
            region: Region::new(scope),
            destination: Destination::Branch {
                region: &mut destination,
                target: target.cloned(),
            },
            fallthrough: true,
        })?;
        destination.ok_or(BuildError::IncompleteBranch)
    }

    fn join_outputs<'a>(
        &self,
        target: &JoinTarget,
        branches: impl IntoIterator<Item = &'a Region>,
    ) -> Result<Vec<usize>, BuildError> {
        let incoming: Vec<_> = branches
            .into_iter()
            .flat_map(|branch| branch.exits_to(target.site))
            .collect();
        if incoming.is_empty() && !target.types.is_empty() {
            return Err(BuildError::MissingBranchValue);
        }
        target
            .types
            .iter()
            .enumerate()
            .map(|(component, &ty)| {
                let inputs: Vec<_> = incoming
                    .iter()
                    .map(|(_, arguments)| arguments[component])
                    .collect();
                self.arena.join_result(ty, target.site, component, &inputs)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
