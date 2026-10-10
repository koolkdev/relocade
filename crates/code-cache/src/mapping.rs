//! The host's canonical mappings and reverse backing aliases.

use std::collections::{HashMap, HashSet};
use wasm86_x86::{ExecutionProfile, PhysicalMemoryMap, CODE_WATCH};

/// One 4-KiB mapping. Offsets name aligned bytes in the guest backing memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mapping {
    Unmapped,
    Ram {
        backing: u32,
        writable: bool,
    },
    /// Available only with physical routing. Fetch snapshots never invoke devices.
    Mmio,
}
impl Mapping {
    pub(crate) fn backing(self) -> Option<u32> {
        match self {
            Self::Ram { backing, .. } => Some(backing),
            _ => None,
        }
    }
}

pub(crate) struct Mappings {
    physical: bool,
    pages: HashMap<u32, Mapping>,
    aliases: HashMap<u32, HashSet<u32>>,
}

impl Mappings {
    pub(crate) fn new(profile: ExecutionProfile, table: &[u8]) -> Self {
        let physical = matches!(profile, ExecutionProfile::Real16);
        let mut result = Self {
            physical,
            pages: HashMap::new(),
            aliases: HashMap::new(),
        };
        let pages = if physical {
            PhysicalMemoryMap::PAGE_COUNT
        } else {
            1 << 20
        };
        for page in 0..pages as u32 {
            let offset = result.offset(page);
            let word = u32::from_le_bytes(table[offset..offset + 4].try_into().unwrap());
            assert_eq!(
                word & CODE_WATCH,
                0,
                "a mapping set has one coherence owner"
            );
            let mapping = if physical {
                match word {
                    0 => Mapping::Unmapped,
                    1 | 2 => Mapping::Ram {
                        backing: u32::from_le_bytes(
                            table[offset + 4..offset + 8].try_into().unwrap(),
                        ),
                        writable: word == 1,
                    },
                    3 => Mapping::Mmio,
                    _ => panic!("invalid physical mapping"),
                }
            } else if word & 1 == 0 {
                Mapping::Unmapped
            } else {
                Mapping::Ram {
                    backing: word & !4095,
                    writable: word & 2 != 0,
                }
            };
            result.replace(page, mapping);
        }
        result
    }

    fn offset(&self, page: u32) -> usize {
        assert!(
            page < if self.physical {
                PhysicalMemoryMap::PAGE_COUNT as u32
            } else {
                1 << 20
            }
        );
        page as usize * if self.physical { 8 } else { 4 }
    }

    pub(crate) fn get(&self, page: u32) -> Mapping {
        self.pages.get(&page).copied().unwrap_or(Mapping::Unmapped)
    }

    pub(crate) fn replace(&mut self, page: u32, mapping: Mapping) {
        if let Some(backing) = self.get(page).backing() {
            let aliases = self.aliases.get_mut(&backing).unwrap();
            aliases.remove(&page);
            if aliases.is_empty() {
                self.aliases.remove(&backing);
            }
        }
        if let Some(backing) = mapping.backing() {
            assert_eq!(backing & 4095, 0, "page-aligned backing");
            self.aliases.entry(backing).or_default().insert(page);
        }
        if mapping == Mapping::Unmapped {
            self.pages.remove(&page);
        } else {
            self.pages.insert(page, mapping);
        }
    }

    pub(crate) fn write(&self, table: &mut [u8], page: u32, watched: bool) {
        let mapping = self.get(page);
        let offset = self.offset(page);
        let mut word = match (self.physical, mapping) {
            (_, Mapping::Unmapped) => 0,
            (true, Mapping::Ram { writable, .. }) => {
                if writable {
                    1
                } else {
                    2
                }
            }
            (true, Mapping::Mmio) => 3,
            (false, Mapping::Ram { backing, writable }) => backing | 1 | (u32::from(writable) << 1),
            (false, Mapping::Mmio) => panic!("virtual mappings do not route devices"),
        };
        if watched {
            word |= CODE_WATCH;
        }
        table[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        if self.physical {
            table[offset + 4..offset + 8]
                .copy_from_slice(&mapping.backing().unwrap_or(0).to_le_bytes());
        }
    }

    pub(crate) fn watch_aliases(&self, table: &mut [u8], backing: u32, watched: bool) {
        if let Some(aliases) = self.aliases.get(&backing) {
            for &page in aliases {
                self.write(table, page, watched);
            }
        }
    }
}
