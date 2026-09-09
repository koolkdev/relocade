//! Multiway branch construction and logical selector validation.
use std::collections::HashSet;

use super::{Region, SwitchCase};
use crate::{AtLeast, BuildError, FunctionBuilder, IntType, IntoOp, Operation, Type, Val, I32};

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
        selector: impl IntoOp<S>,
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
            output: None,
        });
        Ok(())
    }

    /// Executes one arm and joins its value in the parent, using `switch`'s key
    /// matching and construction order. Each arm must consume its builder with
    /// `yield_`, `return_`, `return_void`, `tail_call` or `trap`; at least one arm must yield.
    /// A callback error discards all arms and leaves the parent usable.
    ///
    /// Only the joined result becomes available in the parent. Other values
    /// depending on child reads, calls or joins keep their child scope.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32, I8};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I8], result: Some(Type::I32),
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
    pub fn switch_value<R: IntType, S: IntType>(
        &mut self,
        selector: impl IntoOp<S>,
        cases: &[u32],
        build: impl FnMut(FunctionBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<Val<R>, BuildError>
    where
        I32: AtLeast<S>,
    {
        let selector = self.switch_selector(selector, cases)?;
        let (cases, default) = self.switch_arms(Some(R::TYPE), cases, build)?;
        let output = self.join_output(
            R::TYPE,
            cases
                .iter()
                .map(|case| &case.region)
                .chain(std::iter::once(&default)),
        )?;
        self.region.operations.push(Operation::Switch {
            selector,
            cases,
            default,
            output: Some(output),
        });
        Ok(Val::new(self.arena.clone(), Ok(output)))
    }

    fn switch_selector<S: IntType>(
        &self,
        selector: impl IntoOp<S>,
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
        result: Option<Type>,
        keys: &[u32],
        mut build: impl FnMut(FunctionBuilder<'_>, Option<u32>) -> Result<(), BuildError>,
    ) -> Result<(Vec<SwitchCase>, Region), BuildError> {
        let mut cases = Vec::with_capacity(keys.len());
        for &key in keys {
            let region = self.build_branch(result, |arm| build(arm, Some(key)))?;
            cases.push(SwitchCase { key, region });
        }
        let default = self.build_branch(result, |arm| build(arm, None))?;
        cases.sort_unstable_by_key(|case| case.key);
        Ok((cases, default))
    }
}
