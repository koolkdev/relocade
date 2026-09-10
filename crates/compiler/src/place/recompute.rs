//! Bounded recomputation of calculations inside conditional paths.
use std::collections::BTreeMap;

use super::{representation, Demand, Phase, Point, Tree};
use crate::{control::Site, Body, Operation, ValueKind};

pub(super) fn groups(body: &Body, id: usize, demand: Demand, tree: &Tree<'_>) -> Vec<Demand> {
    // Only these derived operations are nontrapping. Their operands can include
    // snapshots, but loads, calls and joins themselves must never be duplicated.
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
            | ValueKind::Popcnt(_)
            | ValueKind::ZeroTest { .. }
    ) {
        return vec![demand];
    }
    demand
        .exclusive_arms(tree)
        .or_else(|| {
            // Sequential guards can both run. Limit extra work to one additional
            // primitive test, consumed directly by If at every physical demand.
            // Operand calculations keep their ordinary sharing policy.
            let primitive = matches!(
                body.values[id].kind,
                ValueKind::Compare(..) | ValueKind::ZeroTest { .. }
            );
            let selectors_only = demand.points.iter().all(|point| {
                point.phase == Phase::Main
                    && matches!(
                        tree.0[&point.site.region].region.operations.get(point.site.index),
                        Some(Operation::If { condition, .. })
                            if representation(body, *condition) == id
                    )
            });
            (primitive && selectors_only)
                .then(|| demand.guarded_regions(tree))
                .flatten()
        })
        .unwrap_or_else(|| vec![demand])
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

    fn guarded_regions(&self, tree: &Tree<'_>) -> Option<Vec<Self>> {
        let mut regions = BTreeMap::<usize, Self>::new();
        for &point in &self.points {
            let guard = tree.guarded_child(point, self.first.site.region)?;
            include(&mut regions, guard, point, tree);
            if regions.len() > 2 {
                return None;
            }
        }
        (regions.len() == 2).then(|| regions.into_values().collect())
    }
}

fn include(groups: &mut BTreeMap<usize, Demand>, region: usize, point: Point, tree: &Tree<'_>) {
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

    fn guarded_child(&self, point: Point, ancestor: usize) -> Option<usize> {
        let mut region = point.site.region;
        let mut guard = None;
        while region != ancestor {
            let parent = self.0[&region].parent?;
            if matches!(
                self.0[&parent.region].region.operations[parent.index],
                Operation::If { .. } | Operation::Switch { .. }
            ) {
                guard = Some(region);
            }
            region = parent.region;
        }
        // Transparent blocks do not make a calculation conditional. Choose the
        // first actual guarded child below the demands' common ancestor.
        guard
    }
}
