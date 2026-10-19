use super::{short_name, Files, DIRECTORY_ATTRIBUTE, HIDDEN_ATTRIBUTE, SYSTEM_ATTRIBUTE};
use crate::{error::DosError, path};

pub(crate) const FIND_DATA_BYTES: usize = 43;
const VOLUME_ATTRIBUTE: u8 = 0x08;
const ATTRIBUTES_OFFSET: usize = 21;
const DATE_OFFSET: usize = 24;
const SIZE_OFFSET: usize = 26;
const NAME_OFFSET: usize = 30;
const SEARCH_MARKER: u8 = 0xd5;

// DOS reserves the DTA's first 21 bytes for search continuation. Directory IDs
// refer to canonical paths retained for this process; the rest lives in the DTA
// so copying or restoring that buffer preserves an independent search position.
struct Search {
    directory: u32,
    next_entry: u32,
    pattern: [u8; 11],
    attributes: u8,
}

impl Files {
    pub(crate) fn find_first(
        &mut self,
        specification: &[u8],
        attributes: u8,
    ) -> Result<[u8; FIND_DATA_BYTES], DosError> {
        let split = specification
            .iter()
            .rposition(|byte| matches!(byte, b'/' | b'\\' | b':'))
            .map_or(0, |position| position + 1);
        let directory = if split == 0 {
            self.current_directory.clone()
        } else {
            path::resolve(&specification[..split], &self.current_directory)
                .ok_or(DosError::PathNotFound)?
        };
        if !self.filesystem.is_dir(&directory) {
            return Err(DosError::PathNotFound);
        }
        let pattern = short_name::pattern(&specification[split..]).ok_or(DosError::NoMoreFiles)?;
        let directory = match self
            .search_directories
            .iter()
            .position(|path| *path == directory)
        {
            Some(index) => index,
            None => {
                self.search_directories.push(directory);
                self.search_directories.len() - 1
            }
        };
        self.next_match(Search {
            directory: directory as u32,
            next_entry: 0,
            pattern,
            // Read-only and archive do not restrict DOS searches.
            attributes: attributes
                & (HIDDEN_ATTRIBUTE as u8
                    | SYSTEM_ATTRIBUTE as u8
                    | DIRECTORY_ATTRIBUTE
                    | VOLUME_ATTRIBUTE),
        })
    }

    pub(crate) fn find_next(
        &self,
        data: &[u8; FIND_DATA_BYTES],
    ) -> Result<[u8; FIND_DATA_BYTES], DosError> {
        if data[0] != SEARCH_MARKER {
            return Err(DosError::NoMoreFiles);
        }
        self.next_match(Search {
            directory: u32::from_le_bytes(data[1..5].try_into().unwrap()),
            next_entry: u32::from_le_bytes(data[5..9].try_into().unwrap()),
            pattern: data[9..20].try_into().unwrap(),
            attributes: data[20],
        })
    }

    fn next_match(&self, mut search: Search) -> Result<[u8; FIND_DATA_BYTES], DosError> {
        let directory = self
            .search_directories
            .get(search.directory as usize)
            .ok_or(DosError::NoMoreFiles)?;
        if search.attributes == VOLUME_ATTRIBUTE {
            return Err(DosError::NoMoreFiles); // No volume label is configured.
        }
        let dots = [".", ".."]
            .into_iter()
            .filter(|_| !directory.is_empty())
            .map(|name| (name.to_owned(), DIRECTORY_ATTRIBUTE, 0u32));
        let children = self
            .filesystem
            .read_dir(directory)
            .map_err(|_| DosError::NoMoreFiles)?
            .map(|entry| {
                let metadata = entry.metadata();
                let full_path = if directory.is_empty() {
                    entry.file_name().to_owned()
                } else {
                    format!("{directory}/{}", entry.file_name())
                };
                (
                    entry.file_name().to_owned(),
                    self.entry_attributes(&full_path, metadata),
                    metadata.len() as u32,
                )
            });
        for (index, (name, attributes, len)) in dots
            .chain(children)
            .enumerate()
            .skip(search.next_entry as usize)
        {
            let Some(short_name) = short_name::name(name.as_bytes()) else {
                continue;
            };
            let special = attributes
                & (HIDDEN_ATTRIBUTE as u8 | SYSTEM_ATTRIBUTE as u8 | DIRECTORY_ATTRIBUTE);
            if special & !search.attributes != 0
                || !short_name::matches(&search.pattern, &short_name)
            {
                continue;
            }
            search.next_entry = (index + 1) as u32;
            return Ok(search.result(&name, attributes, len));
        }
        Err(DosError::NoMoreFiles)
    }
}

impl Search {
    fn result(&self, name: &str, attributes: u8, len: u32) -> [u8; FIND_DATA_BYTES] {
        let mut data = [0; FIND_DATA_BYTES];
        data[0] = SEARCH_MARKER;
        data[1..5].copy_from_slice(&self.directory.to_le_bytes());
        data[5..9].copy_from_slice(&self.next_entry.to_le_bytes());
        data[9..20].copy_from_slice(&self.pattern);
        data[20] = self.attributes;
        // Public DOS find-data layout: attributes, time, date, size, ASCIZ name.
        data[ATTRIBUTES_OFFSET] = attributes;
        // No clock or file timestamps yet: midnight on 1980-01-01 in DOS format.
        data[DATE_OFFSET..SIZE_OFFSET].copy_from_slice(&0x0021u16.to_le_bytes());
        data[SIZE_OFFSET..NAME_OFFSET].copy_from_slice(&len.to_le_bytes());
        data[NAME_OFFSET..NAME_OFFSET + name.len()].copy_from_slice(name.as_bytes());
        data
    }
}
