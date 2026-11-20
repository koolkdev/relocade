//! Selects compiled instruction spans and dispatch entries around interpreted code.

use super::discovery::successors;
use crate::Result;
use relocade_x86::{
    CodeAddress, DecodedBlock, ExecutionTarget, InstructionFlow, SegmentDefaultSize,
};
use std::collections::{BTreeMap, BTreeSet};

type Location = (Option<u16>, SegmentDefaultSize, u32);

/// Each omitted encoding executes in the interpreter. Its known successors can
/// re-enter compiled code through CPU dispatch, including otherwise private blocks.
pub(super) fn stable_blocks(
    target: ExecutionTarget,
    blocks: Vec<DecodedBlock>,
    mut excluded: impl FnMut(CodeAddress, usize) -> bool,
) -> Result<Vec<DecodedBlock>> {
    let mut stable = Vec::with_capacity(blocks.len());
    let mut resume = BTreeMap::<_, Option<usize>>::new();
    let mut interpreted_fallthrough = Vec::new();
    for block in blocks {
        let start = block.address();
        if !excluded(start, block.byte_len()) {
            stable.push(block);
            continue;
        }
        let exported = block.is_exported();
        let size = block.code_default_size();
        let mut run: Option<DecodedBlock> = None;
        for instruction in block.into_instructions() {
            let eip = instruction.eip();
            let next = instruction.next_eip();
            let address = start.with_eip(eip);
            if excluded(address, next.wrapping_sub(eip) as usize) {
                stable.extend(run.take());
                let flow = instruction.flow(target);
                if flow == InstructionFlow::Continue {
                    interpreted_fallthrough.push((start.segment(), size, next));
                }
                for successor in successors(flow, next, target).into_iter().flatten() {
                    resume.insert((start.segment(), size, successor), None);
                }
            } else if let Some(run) = &mut run {
                run.push(instruction)?;
            } else {
                run = Some(DecodedBlock::new(
                    address,
                    instruction,
                    exported && address == start,
                )?);
            }
        }
        stable.extend(run);
    }
    if resume.is_empty() {
        return Ok(stable);
    }
    if !interpreted_fallthrough.is_empty() {
        add_interpreter_destinations(target, &stable, interpreted_fallthrough, &mut resume);
    }
    let mut blocks: Vec<DecodedBlock> = Vec::with_capacity(stable.len());
    for mut block in stable {
        let address = block.address();
        let key = (address.segment(), block.code_default_size(), address.eip());
        if let Some(slot) = resume.get_mut(&key) {
            block.export();
            // Overlapping explicit snapshots can converge on the same suffix
            // with different instruction limits. Register its longest run once.
            if let Some(index) = *slot {
                let previous = &mut blocks[index];
                if previous.byte_len() < block.byte_len() {
                    *previous = block;
                }
                continue;
            }
            *slot = Some(blocks.len());
        }
        blocks.push(block);
    }
    Ok(blocks)
}

/// The interpreter runs past straight-line instructions until its first control
/// boundary. This can carry it across one or more compiler-selected block splits.
fn add_interpreter_destinations(
    target: ExecutionTarget,
    blocks: &[DecodedBlock],
    mut fallthrough: Vec<Location>,
    resume: &mut BTreeMap<Location, Option<usize>>,
) {
    let tails: BTreeMap<_, _> = blocks
        .iter()
        .map(|block| {
            let address = block.address();
            (
                (address.segment(), block.code_default_size(), address.eip()),
                block.last_instruction(),
            )
        })
        .collect();
    let mut visited = BTreeSet::new();
    while let Some((segment, size, eip)) = fallthrough.pop() {
        if !visited.insert((segment, size, eip)) {
            continue;
        }
        let Some(last) = tails.get(&(segment, size, eip)) else {
            continue;
        };
        let next = last.next_eip();
        let flow = last.flow(target);
        if flow == InstructionFlow::Continue {
            fallthrough.push((segment, size, next));
        } else {
            for successor in successors(flow, next, target).into_iter().flatten() {
                resume.insert((segment, size, successor), None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relocade_x86::{BlockSnapshot, Compiler, ExecutionProfile, SegmentDefaultSize};

    #[test]
    fn overlapping_requests_can_rejoin_the_same_stable_suffix() {
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let code = [
            0xb8, 0x90, 0x90, // MOV AX,9090h; a second entry sees two NOPs
            0xb0, 0x42, // MOV AL,42h; its immediate was repeatedly patched
            0x90, 0xc3, // NOP; RET
        ];
        let snapshot = |offset, instruction_limit| BlockSnapshot {
            address: CodeAddress::CsRelative(offset),
            code_default_size: SegmentDefaultSize::Bits16,
            bytes: &code[offset as usize..],
            instruction_limit,
        };
        let decoded = compiler
            .decode_blocks(&[snapshot(0, 3), snapshot(1, 5)])
            .unwrap();
        let stable = stable_blocks(compiler.target(), decoded, |address, len| {
            (address.eip()..address.eip() + len as u32).contains(&4)
        })
        .unwrap();
        assert!(stable.iter().all(DecodedBlock::is_exported));
        let spans: Vec<_> = stable
            .iter()
            .map(|b| (b.address().eip(), b.byte_len()))
            .collect();
        assert_eq!(spans, [(0, 3), (5, 2), (1, 2)]);
    }
}
