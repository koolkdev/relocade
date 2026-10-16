//! Shared execution backing, including the independently exchangeable CPU image.

use std::mem::{offset_of, size_of};

use super::CpuState;

/// Backing layout of `wasm86.executionContext`, shared by generated entries.
/// Field bytes are little endian on every host. CPU snapshots exchange only
/// `cpu`; host execution controls remain in the enclosing context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionContext {
    pub cpu: CpuState,
    /// Absolute retirement target; `u64::MAX` is practically unbounded.
    /// Budget ordering assumes the instruction counter does not wrap.
    /// Set by the host between entries; generated code only reads it.
    pub instruction_target: u64,
}

impl ExecutionContext {
    pub const BYTE_LEN: usize = size_of::<Self>();

    /// Encodes the complete initial backing. Subsequent CPU replacement writes
    /// only the `cpu` field, preserving execution controls.
    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        let cpu = offset_of!(Self, cpu);
        bytes[cpu..cpu + CpuState::BYTE_LEN].copy_from_slice(&self.cpu.to_bytes());
        let target = offset_of!(Self, instruction_target);
        bytes[target..target + size_of::<u64>()]
            .copy_from_slice(&self.instruction_target.to_le_bytes());
        bytes
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            cpu: CpuState::default(),
            instruction_target: u64::MAX,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_layout_keeps_cpu_snapshot_and_execution_controls_distinct() {
        assert_eq!(offset_of!(ExecutionContext, cpu), 0);
        assert_eq!(offset_of!(ExecutionContext, instruction_target), 488);
        assert_eq!(ExecutionContext::BYTE_LEN, 496);
        let context = ExecutionContext {
            cpu: CpuState::filled(0xa5),
            instruction_target: 0x0807_0605_0403_0201,
        };
        let bytes = context.to_bytes();
        assert_eq!(&bytes[..488], &[0xa5; 488]);
        assert_eq!(&bytes[488..], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&ExecutionContext::default().to_bytes()[488..], &[0xff; 8]);
    }
}
