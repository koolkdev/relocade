//! Shared execution backing, including the independently exchangeable CPU image.

use std::mem::{offset_of, size_of};

use super::CpuState;
use crate::CpuModel;

/// Backing layout of `relocade.executionContext`, shared by generated entries.
/// Field bytes are little endian on every host. CPU snapshots exchange only
/// `cpu`; host execution controls remain in the enclosing context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutionContext {
    pub cpu: CpuState,
    /// Fixed capabilities for this guest. Interpreters read this model at runtime;
    /// specialized blocks must have been compiled for the same model.
    pub cpu_model: CpuModel,
    /// Coalesced execution interrupt request, independent of guest IF. All live
    /// accesses use sequentially consistent byte atomics. Generated checkpoints
    /// only read it; the host owns acknowledgement and the associated work.
    pub interrupt_requested: u8,
}

impl ExecutionContext {
    pub const BYTE_LEN: usize = size_of::<Self>();

    /// Encodes the complete initial backing. Subsequent CPU replacement writes
    /// only the `cpu` field, preserving execution controls.
    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        let cpu = offset_of!(Self, cpu);
        bytes[cpu..cpu + CpuState::BYTE_LEN].copy_from_slice(&self.cpu.to_bytes());
        let model = offset_of!(Self, cpu_model);
        bytes[model..model + size_of::<u32>()]
            .copy_from_slice(&self.cpu_model.bits().to_le_bytes());
        bytes[offset_of!(Self, interrupt_requested)] = self.interrupt_requested;
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_layout_keeps_cpu_snapshot_and_execution_controls_distinct() {
        assert_eq!(offset_of!(ExecutionContext, cpu), 0);
        assert_eq!(offset_of!(ExecutionContext, cpu_model), 536);
        assert_eq!(offset_of!(ExecutionContext, interrupt_requested), 540);
        assert_eq!(ExecutionContext::BYTE_LEN, 544);
        let context = ExecutionContext {
            cpu: CpuState::filled(0xa5),
            cpu_model: CpuModel::default().with_feature(crate::CpuFeature::Cmov, false),
            interrupt_requested: 1,
        };
        let bytes = context.to_bytes();
        assert_eq!(&bytes[..536], &[0xa5; 536]);
        assert_eq!(&bytes[536..], &[0xfd, 1, 0, 0, 1, 0, 0, 0]);
    }
}
