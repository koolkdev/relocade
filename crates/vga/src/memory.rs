use crate::TextCell;

/// Text-mode odd/even CPU addressing selects character and attribute planes.
/// Scanout addresses each 64-KiB plane directly; the CPU aperture is only 32 KiB.
pub(super) struct VideoMemory {
    characters: Box<[u8]>,
    attributes: Box<[u8]>,
}

impl Default for VideoMemory {
    fn default() -> Self {
        Self {
            characters: vec![0; 65536].into_boxed_slice(),
            attributes: vec![0; 65536].into_boxed_slice(),
        }
    }
}

impl VideoMemory {
    pub fn read(&self, offset: u32) -> u8 {
        let index = (offset / 2) as usize;
        if offset & 1 == 0 {
            self.characters[index]
        } else {
            self.attributes[index]
        }
    }

    pub fn write(&mut self, offset: u32, value: u8) {
        let index = (offset / 2) as usize;
        if offset & 1 == 0 {
            self.characters[index] = value;
        } else {
            self.attributes[index] = value;
        }
    }

    pub fn cell(&self, address: u16) -> TextCell {
        TextCell {
            character: self.characters[usize::from(address)],
            attribute: self.attributes[usize::from(address)],
        }
    }
}
