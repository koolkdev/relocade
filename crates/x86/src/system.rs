//! Guest-visible system registers supplied by the embedding environment.

/// Selects a descriptor-table register in the host's system-register view.
/// The discriminants are stable host callback identifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DescriptorTable {
    /// Global descriptor-table register (GDTR).
    Global = 0,
    /// Interrupt descriptor-table register (IDTR).
    Interrupt = 1,
}

/// A descriptor-table register's linear base and inclusive byte limit.
///
/// These values describe the guest environment. They do not point into Wasm
/// backing memory or make [`crate::DescriptorTables`] read packed descriptors.
/// The host must keep this view consistent with its descriptor resolver. In
/// [`crate::ExecutionProfile::Real16`], IDTR is fixed at base zero, limit 0x3ff,
/// matching the interrupt-vector table used by real-mode interrupt delivery.
///
/// The default field types hold native host values. Compiler adapters use the
/// same fields with typed values, preserving the base and limit's logical widths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorTableRegister<Base = u32, Limit = u16> {
    pub base: Base,
    pub limit: Limit,
}

/// Selects the visible selector of a host-managed system segment register.
/// The discriminants are stable host callback identifiers. Hidden descriptor
/// caches and task switching are outside this query contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SystemSelector {
    /// Local descriptor-table register (LDTR).
    Ldtr = 0,
    /// Task register (TR).
    Tr = 1,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_register_identifiers_match_the_host_contract() {
        assert_eq!(DescriptorTable::Global as u32, 0);
        assert_eq!(DescriptorTable::Interrupt as u32, 1);
        assert_eq!(SystemSelector::Ldtr as u32, 0);
        assert_eq!(SystemSelector::Tr as u32, 1);
    }
}
