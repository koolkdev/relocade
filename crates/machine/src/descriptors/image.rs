use super::DescriptorError;
use relocade_x86::{
    CpuState, Descriptor, DescriptorTableRegister, DescriptorTables, SystemSegmentRegister,
};

/// Initial image of the dedicated descriptor address space. CPU table bases are
/// byte offsets in this space, not guest linear addresses. There is no guest
/// page translation or guest access to these bytes.
pub struct DescriptorMemory {
    bytes: Vec<u8>,
    gdtr: DescriptorTableRegister,
    ldtr: SystemSegmentRegister,
}

impl Default for DescriptorMemory {
    fn default() -> Self {
        Self {
            bytes: vec![0; Self::BYTE_LEN as usize],
            gdtr: DescriptorTableRegister::new(0, u16::MAX),
            ldtr: SystemSegmentRegister::default(),
        }
    }
}

impl DescriptorMemory {
    pub const BYTE_LEN: u32 = 2 * 65536;

    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn new(
        tables: DescriptorTables,
        ldt_selector: Option<u16>,
    ) -> Result<Self, DescriptorError> {
        let [mut global, local] = tables.to_packed_tables();
        let ldtr = if let Some(selector) = ldt_selector {
            if selector & !3 == 0 || selector & 4 != 0 {
                return Err(DescriptorError::InvalidLdtSelector);
            }
            if tables.get(selector).is_some() {
                return Err(DescriptorError::OccupiedLdtSlot);
            }
            let offset = usize::from(selector & !7);
            global.resize(global.len().max(offset + Descriptor::BYTE_LEN), 0);
            let descriptor = Descriptor::ldt(65536, u16::MAX);
            global[offset..offset + Descriptor::BYTE_LEN].copy_from_slice(&descriptor.to_bytes());
            SystemSegmentRegister {
                base: 65536,
                limit: u32::from(u16::MAX),
                selector,
                usable: 1,
                access_rights: descriptor.access_rights(),
                ..SystemSegmentRegister::default()
            }
        } else {
            if local.iter().any(|&byte| byte != 0) {
                return Err(DescriptorError::MissingLdtSelector);
            }
            SystemSegmentRegister::default()
        };
        let mut image = Self::default();
        image.bytes[..global.len()].copy_from_slice(&global);
        image.bytes[65536..65536 + local.len()].copy_from_slice(&local);
        image.ldtr = ldtr;
        Ok(image)
    }
    pub(crate) fn initialize_cpu(&self, cpu: &mut CpuState) {
        cpu.gdtr = self.gdtr;
        cpu.ldtr = self.ldtr;
    }
}
