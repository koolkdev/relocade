//! Bounded recomputation of calculations across structured control blocks.
use std::collections::BTreeMap;

use super::{representation, Demand, Phase, Point, Tree};
use crate::{
    body::{Body, Site, ValueDefinition},
    integer::BinaryOp,
    Expression,
};

pub(super) fn groups(body: &Body, id: usize, demand: Demand, tree: &Tree<'_>) -> Vec<Demand> {
    // Derived calculations can be recomputed on their consuming paths. Their
    // operands can include snapshots; loads, calls and joins retain their sharing.
    let ValueDefinition::Expression(expression) = body.values[id].definition else {
        return vec![demand];
    };
    let groups = demand
        .exclusive_arms(tree)
        .or_else(|| {
            // Permit only one split into two sequential placement sites.
            // Alternative arms within either operation do not add another site.
            // Operand demands retain their own sharing and snapshot policies.
            cheap_to_repeat(body, expression)
                .then(|| demand.control_groups(tree))
                .flatten()
        })
        .unwrap_or_else(|| vec![demand]);
    groups
        .into_iter()
        .flat_map(|group| exclusive_groups(group, tree))
        .collect()
}

fn exclusive_groups(demand: Demand, tree: &Tree<'_>) -> Vec<Demand> {
    let mut pending = vec![demand];
    let mut groups = Vec::new();
    while let Some(demand) = pending.pop() {
        if let Some(arms) = demand.exclusive_arms(tree) {
            // Descend through nested alternatives and single-child blocks.
            // Every partition moves the existing demands into child blocks.
            pending.extend(arms.into_iter().rev());
        } else {
            groups.push(demand);
        }
    }
    groups
}

fn cheap_to_repeat(body: &Body, expression: Expression<usize>) -> bool {
    match expression {
        Expression::Binary {
            operator: BinaryOp::Add | BinaryOp::Sub | BinaryOp::And | BinaryOp::Or | BinaryOp::Xor,
            ..
        }
        | Expression::Compare { .. }
        | Expression::ZeroTest { .. }
        | Expression::Normalize { .. }
        | Expression::Convert { .. } => true,
        Expression::Shift { count, .. } => matches!(
            body.values[representation(body, count)].definition,
            ValueDefinition::Constant(_)
        ),
        _ => false,
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum DemandGroup {
    Main,
    Operation(usize),
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

    fn control_groups(&self, tree: &Tree<'_>) -> Option<Vec<Self>> {
        let mut groups = BTreeMap::<DemandGroup, Self>::new();
        for &point in &self.points {
            let group = tree.demand_group(point, self.first.site.block);
            include(&mut groups, group, point, tree);
            if groups.len() > 2 {
                return None;
            }
        }
        (groups.len() == 2).then(|| groups.into_values().collect())
    }
}

fn include<K: Ord>(groups: &mut BTreeMap<K, Demand>, key: K, point: Point, tree: &Tree<'_>) {
    if let Some(demand) = groups.get_mut(&key) {
        demand.include(point, tree);
    } else {
        groups.insert(key, Demand::at(point));
    }
}

impl Tree<'_> {
    fn arm_containing(&self, point: Point, branch: Site) -> Option<usize> {
        let mut block = point.site.block;
        loop {
            let parent = self.0.parent(block)?;
            if parent == branch {
                return Some(block);
            }
            block = parent.block;
        }
    }

    fn demand_group(&self, point: Point, ancestor: usize) -> DemandGroup {
        let mut block = point.site.block;
        while block != ancestor {
            let parent = self
                .0
                .parent(block)
                .expect("a demand descends from its common block");
            if parent.block == ancestor {
                // Alternative arms belong to one operation: only one can run.
                // Blocks count too, since an outward exit can skip their suffix.
                return DemandGroup::Operation(parent.index);
            }
            block = parent.block;
        }
        DemandGroup::Main
    }
}
