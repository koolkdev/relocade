//! DOS MZ headers, relocation and entry state.

use crate::{memory, process, LoadError};
use wasm86_machine::Machine;
use wasm86_x86::{Segment, StoredSegment};

const HEADER_BYTES: usize = 28;
const FILE_PAGE_BYTES: usize = 512;

struct Image<'a> {
    module: &'a [u8],
    relocations: &'a [u8],
    min_extra: u16,
    max_extra: u16,
    ss: u16,
    sp: u16,
    cs: u16,
    ip: u16,
}

impl<'a> Image<'a> {
    fn parse(file: &'a [u8]) -> Result<Self, LoadError> {
        if file.len() < HEADER_BYTES || !matches!(&file[..2], b"MZ" | b"ZM") {
            return Err(LoadError::InvalidMzHeader);
        }
        let last_page_bytes = usize::from(word(file, 0x02));
        let pages = usize::from(word(file, 0x04));
        let relocation_count = usize::from(word(file, 0x06));
        let header_bytes = usize::from(word(file, 0x08)) * memory::PARAGRAPH_BYTES as usize;
        if pages == 0 || last_page_bytes >= FILE_PAGE_BYTES {
            return Err(LoadError::InvalidMzHeader);
        }
        let file_bytes = (pages - 1) * FILE_PAGE_BYTES
            + if last_page_bytes == 0 {
                FILE_PAGE_BYTES
            } else {
                last_page_bytes
            };
        if header_bytes < HEADER_BYTES || header_bytes >= file_bytes || file_bytes > file.len() {
            return Err(LoadError::InvalidMzHeader);
        }
        if word(file, 0x1a) != 0 {
            return Err(LoadError::UnsupportedOverlay);
        }
        let module = &file[header_bytes..file_bytes];
        let relocations = if relocation_count == 0 {
            &[][..]
        } else {
            let start = usize::from(word(file, 0x18));
            let end = start + relocation_count * 4;
            if start < HEADER_BYTES || end > header_bytes {
                return Err(LoadError::InvalidRelocation);
            }
            &file[start..end]
        };
        for relocation in relocations.chunks_exact(4) {
            if relocation_offset(relocation) + 2 > module.len() {
                return Err(LoadError::InvalidRelocation);
            }
        }
        Ok(Self {
            module,
            relocations,
            min_extra: word(file, 0x0a),
            max_extra: word(file, 0x0c),
            ss: word(file, 0x0e),
            sp: word(file, 0x10),
            ip: word(file, 0x14),
            cs: word(file, 0x16),
        })
    }
}

pub(crate) fn load(machine: &mut Machine<'_>, file: &[u8]) -> Result<u16, LoadError> {
    let image = Image::parse(file)?;
    let module_paragraphs = image
        .module
        .len()
        .div_ceil(memory::PARAGRAPH_BYTES as usize);
    let base_paragraphs = usize::from(process::PSP_PARAGRAPHS) + module_paragraphs;
    let available = usize::from(memory::AVAILABLE_PARAGRAPHS);
    if base_paragraphs + usize::from(image.min_extra) > available {
        return Err(LoadError::InsufficientMemory);
    }
    let load_high = image.min_extra == 0 && image.max_extra == 0;
    let paragraphs = if load_high {
        available
    } else {
        // A maximum below the minimum still grants the required minimum.
        (base_paragraphs + usize::from(image.max_extra.max(image.min_extra))).min(available)
    } as u16;

    // All file and allocation checks precede replacing the current process.
    let psp_segment = process::allocate(&mut machine.memory(), paragraphs);
    let load_segment = if load_high {
        psp_segment + paragraphs - module_paragraphs as u16
    } else {
        psp_segment + process::PSP_PARAGRAPHS
    };
    let mut module = image.module.to_vec();
    for relocation in image.relocations.chunks_exact(4) {
        let offset = relocation_offset(relocation);
        let segment = word(&module, offset).wrapping_add(load_segment);
        module[offset..offset + 2].copy_from_slice(&segment.to_le_bytes());
    }
    machine
        .memory()
        .write(u32::from(load_segment) * memory::PARAGRAPH_BYTES, &module)
        .expect("allocated MZ module");
    let mut cpu = process::initial_cpu(psp_segment);
    cpu.segments[Segment::Cs] =
        StoredSegment::real_mode(Segment::Cs, load_segment.wrapping_add(image.cs));
    cpu.segments[Segment::Ss] =
        StoredSegment::real_mode(Segment::Ss, load_segment.wrapping_add(image.ss));
    cpu.eip = u32::from(image.ip);
    cpu.registers.esp = u32::from(image.sp);
    machine
        .cpu()
        .restore(&cpu)
        .expect("real-mode MZ entry state");
    Ok(psp_segment)
}

fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn relocation_offset(entry: &[u8]) -> usize {
    usize::from(word(entry, 2)) * memory::PARAGRAPH_BYTES as usize + usize::from(word(entry, 0))
}
