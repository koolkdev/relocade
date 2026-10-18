//! DOS paragraph allocation. The guest MCB chain is the source of truth.

use crate::error::DosError;
use wasm86_machine::{MachineStorage, Memory};

pub(crate) const PARAGRAPH_BYTES: u32 = 16;
// Keep the IVT and DOS workspace below the single root process.
pub(crate) const FIRST_SEGMENT: u16 = 0x1000;
const FIRST_MCB: u16 = FIRST_SEGMENT - 1;
pub(crate) const END_SEGMENT: u16 = 0xa000; // End of 640-KiB conventional RAM.
pub(crate) const AVAILABLE_PARAGRAPHS: u16 = END_SEGMENT - FIRST_SEGMENT;

struct Block {
    mcb: u16,
    owner: u16,      // PSP segment, or zero for free memory.
    paragraphs: u16, // Excludes the MCB's own paragraph.
}

pub(crate) fn reset(memory: &mut Memory<'_, impl MachineStorage + ?Sized>) {
    write_chain(
        memory,
        &[Block {
            mcb: FIRST_MCB,
            owner: 0,
            paragraphs: AVAILABLE_PARAGRAPHS,
        }],
    );
}

pub(crate) fn allocate(
    memory: &mut Memory<'_, impl MachineStorage + ?Sized>,
    paragraphs: u16,
    owner: u16,
) -> Result<u16, DosError> {
    let mut blocks = read_chain(memory)?;
    coalesce(&mut blocks);
    let Some(index) = blocks
        .iter()
        .position(|block| block.owner == 0 && block.paragraphs >= paragraphs)
    else {
        let available = blocks
            .iter()
            .filter(|block| block.owner == 0)
            .map(|block| block.paragraphs)
            .max()
            .unwrap_or(0);
        return Err(DosError::InsufficientMemory { available });
    };
    split(&mut blocks, index, paragraphs);
    blocks[index].owner = owner;
    let segment = blocks[index].mcb + 1;
    write_chain(memory, &blocks);
    Ok(segment)
}

pub(crate) fn free(
    memory: &mut Memory<'_, impl MachineStorage + ?Sized>,
    segment: u16,
) -> Result<(), DosError> {
    let mut blocks = read_chain(memory)?;
    let index = allocated_block(&blocks, segment)?;
    blocks[index].owner = 0;
    coalesce(&mut blocks);
    write_chain(memory, &blocks);
    Ok(())
}

pub(crate) fn resize(
    memory: &mut Memory<'_, impl MachineStorage + ?Sized>,
    segment: u16,
    paragraphs: u16,
) -> Result<(), DosError> {
    let mut blocks = read_chain(memory)?;
    coalesce(&mut blocks);
    let index = allocated_block(&blocks, segment)?;
    if paragraphs > blocks[index].paragraphs
        && blocks.get(index + 1).is_some_and(|block| block.owner == 0)
    {
        let next = blocks.remove(index + 1);
        blocks[index].paragraphs += 1 + next.paragraphs;
    }
    let available = blocks[index].paragraphs;
    let result = if paragraphs > available {
        // DOS grows the block as far as possible even when the request fails.
        Err(DosError::InsufficientMemory { available })
    } else {
        split(&mut blocks, index, paragraphs);
        Ok(())
    };
    coalesce(&mut blocks);
    write_chain(memory, &blocks);
    result
}

fn allocated_block(blocks: &[Block], segment: u16) -> Result<usize, DosError> {
    blocks
        .iter()
        .position(|block| block.mcb + 1 == segment && block.owner != 0)
        .ok_or(DosError::InvalidMemoryBlock)
}

fn split(blocks: &mut Vec<Block>, index: usize, paragraphs: u16) {
    let block = &mut blocks[index];
    if paragraphs < block.paragraphs {
        let remainder = Block {
            mcb: block.mcb + 1 + paragraphs,
            owner: 0,
            paragraphs: block.paragraphs - paragraphs - 1,
        };
        block.paragraphs = paragraphs;
        blocks.insert(index + 1, remainder);
    }
}

fn coalesce(blocks: &mut Vec<Block>) {
    let mut index = 0;
    while index + 1 < blocks.len() {
        if blocks[index].owner == 0 && blocks[index + 1].owner == 0 {
            let next = blocks.remove(index + 1);
            blocks[index].paragraphs += 1 + next.paragraphs;
        } else {
            index += 1;
        }
    }
}

fn read_chain(
    memory: &mut Memory<'_, impl MachineStorage + ?Sized>,
) -> Result<Vec<Block>, DosError> {
    let mut blocks = Vec::new();
    let mut mcb = FIRST_MCB;
    loop {
        // MCB offsets: 0 = 'M'/'Z' marker, 1 = owner PSP, 3 = paragraph count.
        let mut header = [0; 5];
        memory
            .read(u32::from(mcb) * PARAGRAPH_BYTES, &mut header)
            .expect("conventional memory");
        let paragraphs = u16::from_le_bytes([header[3], header[4]]);
        let next = u32::from(mcb) + 1 + u32::from(paragraphs);
        let last = header[0] == b'Z';
        if !matches!(header[0], b'M' | b'Z')
            || next > u32::from(END_SEGMENT)
            || last != (next == u32::from(END_SEGMENT))
        {
            return Err(DosError::MemoryCorrupted);
        }
        blocks.push(Block {
            mcb,
            owner: u16::from_le_bytes([header[1], header[2]]),
            paragraphs,
        });
        if last {
            return Ok(blocks);
        }
        mcb = next as u16;
    }
}

fn write_chain(memory: &mut Memory<'_, impl MachineStorage + ?Sized>, blocks: &[Block]) {
    for (index, block) in blocks.iter().enumerate() {
        let [owner_lo, owner_hi] = block.owner.to_le_bytes();
        let [size_lo, size_hi] = block.paragraphs.to_le_bytes();
        let marker = if index + 1 == blocks.len() {
            b'Z'
        } else {
            b'M'
        };
        memory
            .write(
                u32::from(block.mcb) * PARAGRAPH_BYTES,
                &[marker, owner_lo, owner_hi, size_lo, size_hi],
            )
            .expect("conventional memory");
    }
}
