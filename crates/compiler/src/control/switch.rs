//! Multiway branch construction and logical selector validation.
use std::collections::HashSet;

use super::{JoinTarget, Region, SwitchCase};
use crate::{AtLeast, BuildError, FunctionBuilder, IntType, Operation, Results, Val, I32};

impl FunctionBuilder<'_> {
    /// Executes the arm whose key equals the selector, or the default arm when no
    /// key matches. Keys are unsigned and must be unique and fit the selector's
    /// logical type. The selector may be I1, I8, I16 or I32.
    ///
    /// Construction invokes `build` once per key in the supplied order with
    /// `Some(key)`, then once with `None` for the default. An empty case list builds
    /// only the default. Each arm may fall through, return, tail-call or trap.
    /// A callback error discards all arms; child values follow `if_`'s scope rules.
    /// Dense key ranges use a branch table; sparse ranges need no large table.
    pub fn switch<S: IntType>(
        &mut self,
        selector: impl Into<Val<S>>,
        cases: &[u32],
        build: impl FnMut(FunctionBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<S>,
    {
        let selector = self.switch_selector(selector, cases)?;
        let (cases, default) = self.switch_arms(None, cases, build)?;
        self.region.operations.push(Operation::Switch {
            selector,
            cases,
            default,
            outputs: Vec::new(),
        });
        Ok(())
    }

    /// Executes one arm and joins its typed result in the parent, using `switch`'s
    /// key matching and construction order. Nonempty result arms must consume
    /// their builder with `yield_`, an outward `branch`, `return_`,
    /// `tail_call` or `trap`; at least one must yield to this switch. Unit result
    /// arms may fall through.
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
        build: impl FnMut(FunctionBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError>
    where
        I32: AtLeast<S>,
    {
        let selector = self.switch_selector(selector, cases)?;
        let target = self.result_target::<R>();
        let (cases, default) = self.switch_arms(Some(&target), cases, build)?;
        let outputs = self.join_outputs(
            &target,
            cases
                .iter()
                .map(|case| &case.region)
                .chain(std::iter::once(&default)),
        )?;
        let values = crate::results::bind::<R>(self, &outputs);
        self.region.operations.push(Operation::Switch {
            selector,
            cases,
            default,
            outputs,
        });
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
        mut build: impl FnMut(FunctionBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<(Vec<SwitchCase>, Region), BuildError> {
        let mut cases = Vec::with_capacity(keys.len());
        for &key in keys {
            let region = self.build_branch(target, |arm| build(arm, Some(key)))?;
            cases.push(SwitchCase { key, region });
        }
        let default = self.build_branch(target, |arm| build(arm, None))?;
        cases.sort_unstable_by_key(|case| case.key);
        Ok((cases, default))
    }
}
