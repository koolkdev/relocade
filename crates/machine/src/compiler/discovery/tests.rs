mod exports;
mod limits;
mod paths;
mod shared;

use super::BlockDiscovery;
use crate::{DiscoveryError, DiscoveryLimits, Error};
use relocade_x86::{
    BlockError, CodeAddress, Compiler, DecodedBlock, ExecutionProfile, SegmentDefaultSize,
    SegmentProfile,
};
use SegmentDefaultSize::{Bits16, Bits32};

const START: u32 = 0x1000;

fn address(eip: u32) -> CodeAddress {
    CodeAddress::CsRelative(eip)
}

fn mapped_code<'a, const N: usize>(
    ranges: [(CodeAddress, &'a [u8]); N],
) -> impl FnMut(CodeAddress, &mut [u8]) -> usize + 'a {
    move |address, output| {
        for (start, bytes) in ranges {
            if start.segment() != address.segment() {
                continue;
            }
            let offset = address.eip().wrapping_sub(start.eip()) as usize;
            if let Some(bytes) = bytes.get(offset..).filter(|bytes| !bytes.is_empty()) {
                let len = output.len().min(bytes.len());
                output[..len].copy_from_slice(&bytes[..len]);
                return len;
            }
        }
        0
    }
}

fn discover(
    profile: ExecutionProfile,
    size: SegmentDefaultSize,
    code: &[u8],
    limits: DiscoveryLimits,
) -> crate::Result<Vec<DecodedBlock>> {
    let root = address(START);
    let target = Compiler::new(profile).config();
    let mut discovery = BlockDiscovery::new(target, mapped_code([(root, code)]));
    discovery.add_root(root, size, limits)?;
    discovery.finish()
}

fn spans(blocks: &[DecodedBlock]) -> Vec<(u32, usize)> {
    blocks
        .iter()
        .map(|block| (block.address().eip(), block.byte_len()))
        .collect()
}
