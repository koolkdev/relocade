//! Placement of reads and shared values among ordered stores.
use wasm_encoder::ValType;

use crate::{emit::wasm_type, Body, Operation, ValueKind};

pub(super) struct Placement {
    pub(super) slots: Vec<Option<usize>>,
    pub(super) captures: Vec<bool>,
    pub(super) slot_types: Vec<ValType>,
}

#[derive(Clone, Copy)]
struct Demand {
    first: usize,
    count: usize,
}

fn demand(demands: &mut [Option<Demand>], value: usize, site: usize) {
    let entry = demands[value].get_or_insert(Demand {
        first: site,
        count: 0,
    });
    entry.first = entry.first.min(site);
    entry.count += 1;
}

pub(super) fn plan(body: &Body) -> Placement {
    let mut demands = vec![None; body.values.len()];
    for (site, operation) in body.operations.iter().enumerate() {
        if let Operation::Store { value, .. } = *operation {
            demand(&mut demands, value, site);
        }
    }
    demand(&mut demands, body.result, body.operations.len());
    let mut captures = vec![false; body.values.len()];
    // Operands precede consumers. A shared addition executes at its first use,
    // so each input is needed once there, even if the addition has later uses.
    for id in (0..body.values.len()).rev() {
        let Some(use_) = demands[id] else { continue };
        match body.values[id].kind {
            ValueKind::Add(a, b) => {
                demand(&mut demands, a, use_.first);
                demand(&mut demands, b, use_.first);
            }
            ValueKind::Load { location, site } => {
                // The store consuming a load runs after its operands. Only a
                // write strictly before that first use can destroy the snapshot.
                captures[id] = body.operations[site + 1..use_.first]
                    .iter()
                    .any(|operation| {
                        matches!(operation, Operation::Store { location: other, .. }
                        if location.overlaps(*other))
                    });
            }
            ValueKind::Constant(_) | ValueKind::Parameter(_) => {}
        }
    }
    let mut slot_types = Vec::new();
    let slots = body
        .values
        .iter()
        .enumerate()
        .map(|(id, value)| {
            if demands[id].is_some_and(|use_| use_.count > 1 || captures[id])
                && matches!(value.kind, ValueKind::Add(..) | ValueKind::Load { .. })
            {
                let slot = slot_types.len();
                slot_types.push(wasm_type(value.ty));
                Some(slot)
            } else {
                None
            }
        })
        .collect();
    Placement {
        slots,
        captures,
        slot_types,
    }
}
