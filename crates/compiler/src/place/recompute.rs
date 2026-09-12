//! Bounded recomputation of calculations across structured control regions.
use std::collections::BTreeMap;

use super::{representation, Demand, Phase, Point, Tree};
use crate::{control::Site, integer::BinaryOp, Body, ValueKind};

pub(super) fn groups(body: &Body, id: usize, demand: Demand, tree: &Tree<'_>) -> Vec<Demand> {
    // Derived calculations can be recomputed on their consuming paths. Their
    // operands can include snapshots; loads, calls and joins retain their sharing.
    if !matches!(
        body.values[id].kind,
        ValueKind::Binary(..)
            | ValueKind::Compare(..)
            | ValueKind::Shift { .. }
            | ValueKind::Rotate { .. }
            | ValueKind::Select { .. }
            | ValueKind::Normalize(_)
            | ValueKind::Convert(_)
            | ValueKind::SignExtend(_)
            | ValueKind::BitCount(..)
            | ValueKind::ZeroTest { .. }
    ) {
        return vec![demand];
    }
    demand
        .exclusive_arms(tree)
        .or_else(|| {
            // These regions can run on the same path. Permit at most two
            // placements of each cheap value, not recursive copies of its DAG.
            // Operand demands retain their own sharing and snapshot policies.
            cheap_to_repeat(body, id)
                .then(|| demand.control_regions(tree))
                .flatten()
        })
        .unwrap_or_else(|| vec![demand])
}

fn cheap_to_repeat(body: &Body, id: usize) -> bool {
    match body.values[id].kind {
        ValueKind::Binary(
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::And | BinaryOp::Or | BinaryOp::Xor,
            ..,
        )
        | ValueKind::Compare(..)
        | ValueKind::ZeroTest { .. }
        | ValueKind::Normalize(_)
        | ValueKind::Convert(_) => true,
        ValueKind::Shift { count, .. } => matches!(
            body.values[representation(body, count)].kind,
            ValueKind::Constant(_)
        ),
        _ => false,
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum DemandRegion {
    Main,
    Child(usize),
}

impl Demand {
    fn exclusive_arms(&self, tree: &Tree<'_>) -> Option<Vec<Self>> {
        if self.first != self.last || self.first.phase != Phase::Header || self.at_first {
            return None;
        }
        // A common header is only a bound. Split actual uses by its direct child,
        // then choose normal placement within each mutually exclusive arm.
        let mut arms = BTreeMap::<usize, Self>::new();
        for &point in &self.points {
            let arm = tree.arm_containing(point, self.first.site)?;
            include(&mut arms, arm, point, tree);
        }
        Some(arms.into_values().collect())
    }

    fn control_regions(&self, tree: &Tree<'_>) -> Option<Vec<Self>> {
        let mut regions = BTreeMap::<DemandRegion, Self>::new();
        for &point in &self.points {
            let region = tree.demand_region(point, self.first.site.region);
            include(&mut regions, region, point, tree);
            if regions.len() > 2 {
                return None;
            }
        }
        (regions.len() == 2).then(|| regions.into_values().collect())
    }
}

fn include<K: Ord>(groups: &mut BTreeMap<K, Demand>, region: K, point: Point, tree: &Tree<'_>) {
    if let Some(demand) = groups.get_mut(&region) {
        demand.include(point, tree);
    } else {
        groups.insert(region, Demand::at(point));
    }
}

impl Tree<'_> {
    fn arm_containing(&self, point: Point, branch: Site) -> Option<usize> {
        let mut region = point.site.region;
        loop {
            let parent = self.0[&region].parent?;
            if parent == branch {
                return Some(region);
            }
            region = parent.region;
        }
    }

    fn demand_region(&self, point: Point, ancestor: usize) -> DemandRegion {
        let mut region = point.site.region;
        while region != ancestor {
            let parent = self.0[&region]
                .parent
                .expect("a demand descends from its common region");
            if parent.region == ancestor {
                // Blocks count too: an outward exit can skip a use in their
                // suffix. Repeated uses within this child still share normally.
                return DemandRegion::Child(region);
            }
            region = parent.region;
        }
        DemandRegion::Main
    }
}
