//! Reuse of Wasm locals between values whose accesses do not overlap.
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use wasm_encoder::ValType;

pub(super) struct AllocatedLocals {
    pub(super) types: Vec<ValType>,
    pub(super) indices: Vec<Option<u32>>,
}

pub(super) fn allocate(
    accesses: impl IntoIterator<Item = usize>,
    slot_types: &[ValType],
) -> AllocatedLocals {
    let mut lifetimes = vec![None; slot_types.len()];
    for (position, slot) in accesses.into_iter().enumerate() {
        let lifetime = lifetimes[slot].get_or_insert((position, position));
        lifetime.1 = position;
    }
    let mut intervals: Vec<_> = lifetimes
        .into_iter()
        .enumerate()
        // Stack forwarding can leave a planned slot without any local access.
        .filter_map(|(slot, lifetime)| lifetime.map(|(first, last)| (first, last, slot)))
        .collect();
    intervals.sort_unstable_by_key(|&(first, _, slot)| (first, slot));

    let mut types = Vec::new();
    let mut indices = vec![None; slot_types.len()];
    let mut active = BinaryHeap::<Reverse<(usize, u32)>>::new();
    let mut free = [BinaryHeap::<Reverse<u32>>::new(), BinaryHeap::new()];
    // A local must retain its value through its last access. After that, another
    // value of the same type can use it, even if the old value is still on the stack.
    for (first, last, slot) in intervals {
        while let Some(&Reverse((end, local))) = active.peek() {
            if end >= first {
                break;
            }
            active.pop();
            free[type_index(types[local as usize])].push(Reverse(local));
        }
        let ty = slot_types[slot];
        let local = if let Some(Reverse(local)) = free[type_index(ty)].pop() {
            local
        } else {
            let local = u32::try_from(types.len()).expect("Wasm local index overflow");
            types.push(ty);
            local
        };
        indices[slot] = Some(local);
        active.push(Reverse((last, local)));
    }
    AllocatedLocals { types, indices }
}

fn type_index(ty: ValType) -> usize {
    match ty {
        ValType::I32 => 0,
        ValType::I64 => 1,
        _ => unreachable!("only integer carriers reach local allocation"),
    }
}

#[cfg(test)]
mod tests {
    use super::allocate;
    use wasm_encoder::ValType;

    #[test]
    fn disjoint_lifetimes_reuse_a_local_of_the_same_type() {
        for ty in [ValType::I32, ValType::I64] {
            let allocated = allocate([0, 0, 1, 1], &[ty, ty]);
            assert_eq!(allocated.indices[0], allocated.indices[1]);
            assert_eq!(allocated.types.len(), 1);
            for local in allocated.indices {
                assert_eq!(allocated.types[local.unwrap() as usize], ty);
            }
        }
    }

    #[test]
    fn narrow_and_i32_values_reuse_the_same_carrier_local() {
        use crate::{emit::wasm_type, Type};

        let types = [Type::I1, Type::I8, Type::I16, Type::I32].map(wasm_type);
        let allocated = allocate([0, 0, 1, 1, 2, 2, 3, 3], &types);
        assert_eq!(allocated.types, [ValType::I32]);
        for local in &allocated.indices {
            assert_eq!(*local, allocated.indices[0]);
        }
    }

    #[test]
    fn overlapping_or_different_types_need_distinct_locals() {
        let types = [ValType::I32, ValType::I32, ValType::I64];
        let allocated = allocate([0, 1, 0, 1, 2, 2], &types);
        assert_ne!(allocated.indices[0], allocated.indices[1]);
        assert_ne!(allocated.indices[0], allocated.indices[2]);
        assert_ne!(allocated.indices[1], allocated.indices[2]);
        assert_eq!(allocated.types.len(), types.len());
        for (slot, ty) in types.into_iter().enumerate() {
            assert_eq!(
                allocated.types[allocated.indices[slot].unwrap() as usize],
                ty
            );
        }
    }
}
