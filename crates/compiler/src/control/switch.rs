//! Multiway branch construction and logical selector validation.
use std::collections::HashSet;

use super::JoinTarget;
use crate::{
    body::{Edge, Exit, Layout},
    function::PendingBlock,
    AtLeast, BlockBuilder, BuildError, IntType, Results, Val, I32,
};

impl BlockBuilder<'_> {
    /// Executes the arm whose key equals the selector, or the default arm when no
    /// key matches. Keys are unsigned and must be unique and fit the selector's
    /// logical type. The selector may be I1, I8, I16 or I32.
    ///
    /// For a constant selector construction invokes only the matching arm, or
    /// the default when no key matches. Otherwise it invokes `build` once per key
    /// in the supplied order with `Some(key)`, then once with `None` for the default.
    /// An empty case list builds only the default. Each arm may fall through,
    /// return, tail-call or trap.
    /// Skipped closures follow [`Self::if_`]'s construction contract.
    /// A callback error discards all arms; child values follow `if_`'s scope rules.
    /// Dense key ranges use a branch table; sparse ranges need no large table.
    pub fn switch<S: IntType>(
        &mut self,
        selector: impl Into<Val<S>>,
        cases: &[u32],
        mut build: impl FnMut(BlockBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<S>,
    {
        let selector = self.switch_selector(selector, cases)?;
        if let Some(bits) = self.arena.constant_bits(selector)? {
            let key = cases.iter().copied().find(|&key| u64::from(key) == bits);
            let branch = self.build_branch(None, |arm| build(arm, key))?;
            let continuation = self.arena.block(self.pending.id, &[])?;
            let path = branch.path.clone();
            self.attach_scope(branch, continuation)?;
            self.pending.path = path;
            return Ok(());
        }
        let (cases, default) = self.switch_arms(None, cases, build)?;
        let continuation = self.arena.block(self.pending.id, &[])?;
        self.attach_switch(selector, cases, default, continuation)?;
        Ok(())
    }

    /// Executes one arm and joins its typed result in the parent, using `switch`'s
    /// key matching and construction order. Nonempty result arms must consume
    /// their builder with `yield_`, an outward `branch`, `return_`,
    /// `tail_call` or `trap`. If every constructed arm exits elsewhere, the result
    /// and continuation are unreachable. Unit result arms may fall through.
    /// A callback error discards all arms and leaves the parent usable.
    ///
    /// Only the joined result becomes available in the parent. Other values
    /// depending on child reads, calls or joins keep their child scope.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32, I8};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I8], results: vec![Type::I32],
    /// }, |mut body| {
    ///     let selector = body.parameter::<I8>(0)?;
    ///     let value = body.switch_value::<I32, _>(&selector, &[2, 5], |arm, key| {
    ///         arm.yield_(match key { Some(2) => 20, Some(5) => 50, _ => 0 })
    ///     })?;
    ///     body.return_(value)
    /// })?;
    /// program.export("choose", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn switch_value<R: Results, S: IntType>(
        &mut self,
        selector: impl Into<Val<S>>,
        cases: &[u32],
        mut build: impl FnMut(BlockBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError>
    where
        I32: AtLeast<S>,
    {
        let selector = self.switch_selector(selector, cases)?;
        let target = self.result_target::<R>()?;
        if let Some(bits) = self.arena.constant_bits(selector)? {
            let key = cases.iter().copied().find(|&key| u64::from(key) == bits);
            let branch = self.build_branch(Some(&target), |arm| build(arm, key))?;
            let outputs = self.join_outputs(&target, &[branch.entry])?;
            let values = crate::results::bind::<R>(self, &outputs);
            self.attach_scope(branch, target.target)?;
            return Ok(values);
        }
        let (cases, default) = self.switch_arms(Some(&target), cases, build)?;
        let entries: Vec<_> = cases
            .iter()
            .map(|(_, block)| block.entry)
            .chain(std::iter::once(default.entry))
            .collect();
        let outputs = self.join_outputs(&target, &entries)?;
        let values = crate::results::bind::<R>(self, &outputs);
        self.attach_switch(selector, cases, default, target.target)?;
        Ok(values)
    }

    fn switch_selector<S: IntType>(
        &self,
        selector: impl Into<Val<S>>,
        cases: &[u32],
    ) -> Result<usize, BuildError>
    where
        I32: AtLeast<S>,
    {
        let selector = self.operand(selector)?;
        let mut seen = HashSet::with_capacity(cases.len());
        for &key in cases {
            if u64::from(key) > S::TYPE.mask() {
                return Err(BuildError::SwitchCaseOutOfRange {
                    key,
                    selector: S::TYPE,
                });
            }
            if !seen.insert(key) {
                return Err(BuildError::DuplicateSwitchCase { key });
            }
        }
        self.arena.normalize(selector)
    }

    fn switch_arms(
        &mut self,
        target: Option<&JoinTarget>,
        keys: &[u32],
        mut build: impl FnMut(BlockBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<(Vec<(u32, PendingBlock)>, PendingBlock), BuildError> {
        let mut cases = Vec::with_capacity(keys.len());
        for &key in keys {
            let block = self.build_branch(target, |arm| build(arm, Some(key)))?;
            cases.push((key, block));
        }
        let default = self.build_branch(target, |arm| build(arm, None))?;
        cases.sort_unstable_by_key(|(key, _)| *key);
        Ok((cases, default))
    }
    fn attach_switch(
        &mut self,
        selector: usize,
        cases: Vec<(u32, PendingBlock)>,
        default: PendingBlock,
        join: crate::body::BlockId,
    ) -> Result<(), BuildError> {
        for (_, branch) in &cases {
            self.connect_fallthrough(branch, join)?;
        }
        self.connect_fallthrough(&default, join)?;
        self.arena.exit(
            self.pending.current,
            Exit::Switch {
                selector,
                cases: cases
                    .iter()
                    .map(|(key, block)| {
                        (
                            *key,
                            Edge {
                                target: block.entry,
                                arguments: Vec::new(),
                            },
                        )
                    })
                    .collect(),
                default: Edge {
                    target: default.entry,
                    arguments: Vec::new(),
                },
            },
        )?;
        self.pending.layout.push(Layout::Switch {
            branch: self.pending.current,
            cases: cases
                .into_iter()
                .map(|(key, block)| (key, block.layout))
                .collect(),
            default: default.layout,
            join,
        });
        self.pending.current = join;
        Ok(())
    }
}
