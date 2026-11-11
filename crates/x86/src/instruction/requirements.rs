//! Admission shared by block boundaries and instruction execution.

use crate::{processor, CpuFeature, Exception, ExecutionProfile, ExecutionTarget};
use relocade_compiler::{Val, I1, I8};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Availability {
    AllModes,
    ProtectedOnly,
    /// Requires CPL0; supported protected profiles execute at CPL3.
    Privileged,
    /// Requires CPL <= IOPL; supported protected profiles fix CPL3/IOPL0.
    IoPrivileged,
}

/// Checked after complete decoding, before operand access or instruction effects.
#[derive(Clone, Copy)]
pub(super) struct Requirements {
    pub(super) availability: Availability,
    pub(super) features: &'static [CpuFeature],
}

impl Requirements {
    pub(super) fn fault(self, target: ExecutionTarget) -> Option<Exception> {
        if processor::supports_features(target.cpu_model.bits(), self.features)
            .constant_bits()
            .expect("CPU feature masks are constants")
            == Some(0)
        {
            return Some(Exception::InvalidOpcode);
        }
        let protected = matches!(target.profile, ExecutionProfile::Protected(_));
        self.mode_fault(
            protected.into(),
            (if protected { 3 } else { 0 }).into(),
            0.into(),
        )
        .and_then(|(condition, exception)| {
            (condition
                .constant_bits()
                .expect("decoding properties are constants")
                == Some(1))
            .then_some(exception)
        })
    }

    pub(super) fn mode_fault(
        self,
        protected: Val<I1>,
        privilege: Val<I8>,
        iopl: Val<I8>,
    ) -> Option<(Val<I1>, Exception)> {
        match self.availability {
            Availability::AllModes => None,
            Availability::ProtectedOnly => Some((protected.eq(false), Exception::InvalidOpcode)),
            Availability::Privileged => Some((
                privilege.ne(0),
                Exception::GeneralProtection { error_code: 0 },
            )),
            Availability::IoPrivileged => Some((
                iopl.unsigned().lt(privilege),
                Exception::GeneralProtection { error_code: 0 },
            )),
        }
    }
}
