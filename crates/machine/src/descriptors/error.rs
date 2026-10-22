use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    OutsideStorage,
    OutsideTable,
    InvalidLdtSelector,
    OccupiedLdtSlot,
    MissingLdtSelector,
    RealMode,
}
impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OutsideStorage => "descriptor exceeds dedicated storage",
            Self::OutsideTable => "selector is outside the active descriptor table",
            Self::InvalidLdtSelector => "LDTR requires a nonnull GDT selector",
            Self::OccupiedLdtSlot => "LDTR selector would overwrite a GDT descriptor",
            Self::MissingLdtSelector => "local descriptors require an LDTR selector",
            Self::RealMode => "descriptor table installation requires protected user mode",
        })
    }
}
impl std::error::Error for DescriptorError {}
