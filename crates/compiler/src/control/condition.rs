//! Prediction belongs to a control-flow use, not to its boolean value.

use crate::{Val, I1};

/// The expected outcome of a condition. Engines may ignore this prediction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BranchHint {
    /// The condition is usually true.
    Likely,
    /// The condition is usually false.
    Unlikely,
}

/// A boolean condition with an optional prediction for this branch.
///
/// Conditional builders accept ordinary boolean values without a prediction.
/// Use [`Val::likely`] or [`Val::unlikely`] to predict an outcome. Hints do not
/// change execution, value placement, validation or constant folding. Only
/// surviving branches receive WebAssembly `metadata.code.branch_hint` entries;
/// engines that do not support that custom section can ignore it.
///
/// ```
/// use wasm86_compiler::{Program, Signature, Type, I32};
/// let mut program = Program::new();
/// let function = program.function(Signature {
///     parameters: vec![Type::I32], results: vec![Type::I32],
/// }, |mut body| {
///     let input = body.parameter::<I32>(0)?;
///     body.if_(input.eq(0).unlikely(), |arm| arm.return_(7))?;
///     body.return_(input)
/// })?;
/// # Ok::<(), wasm86_compiler::BuildError>(())
/// ```
#[derive(Clone)]
pub struct BranchCondition {
    pub(super) value: Val<I1>,
    pub(super) hint: Option<BranchHint>,
}

impl<T: Into<Val<I1>>> From<T> for BranchCondition {
    fn from(value: T) -> Self {
        Self {
            value: value.into(),
            hint: None,
        }
    }
}

impl BranchCondition {
    /// Predicts this condition's outcome without changing its value.
    pub fn with_hint(mut self, hint: BranchHint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl Val<I1> {
    /// Predicts that a branch using this condition will take its true arm.
    pub fn likely(&self) -> BranchCondition {
        BranchCondition::from(self).with_hint(BranchHint::Likely)
    }

    /// Predicts that a branch using this condition will take its false arm.
    pub fn unlikely(&self) -> BranchCondition {
        BranchCondition::from(self).with_hint(BranchHint::Unlikely)
    }
}
