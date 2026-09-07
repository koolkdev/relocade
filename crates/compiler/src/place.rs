//! Placement of reads and shared values among structured effects.
use std::collections::HashMap;

use wasm_encoder::ValType;

use crate::{
    control::{Region, Site},
    emit::wasm_type,
    memory::Location,
    Body, Operation, ValueKind,
};

pub(super) struct Placement {
    pub(super) slots: Vec<Option<usize>>,
    pub(super) captures: HashMap<Site, Vec<usize>>,
    pub(super) slot_types: Vec<ValType>,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Phase {
    Main,
    Header,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Point {
    site: Site,
    phase: Phase,
}

impl Point {
    fn main(site: Site) -> Self {
        Self {
            site,
            phase: Phase::Main,
        }
    }
}

struct RegionInfo<'a> {
    region: &'a Region,
    parent: Option<Site>,
    depth: usize,
}

struct Tree<'a>(HashMap<usize, RegionInfo<'a>>);

impl<'a> Tree<'a> {
    fn new(root: &'a Region) -> Self {
        let mut regions = HashMap::new();
        let mut pending = vec![(root, None, 0)];
        while let Some((region, parent, depth)) = pending.pop() {
            for (index, operation) in region.operations.iter().enumerate() {
                if let Operation::If { branch, .. } = operation {
                    pending.push((
                        branch,
                        Some(Site {
                            region: region.id,
                            index,
                        }),
                        depth + 1,
                    ));
                }
            }
            regions.insert(
                region.id,
                RegionInfo {
                    region,
                    parent,
                    depth,
                },
            );
        }
        Self(regions)
    }

    fn lift(&self, point: Point) -> Point {
        Point {
            site: self.0[&point.site.region]
                .parent
                .expect("a child has a parent"),
            phase: Phase::Header,
        }
    }

    fn common(&self, mut a: Point, mut b: Point) -> Point {
        while self.0[&a.site.region].depth > self.0[&b.site.region].depth {
            a = self.lift(a);
        }
        while self.0[&b.site.region].depth > self.0[&a.site.region].depth {
            b = self.lift(b);
        }
        while a.site.region != b.site.region {
            a = self.lift(a);
            b = self.lift(b);
        }
        // A child cannot initialize values for its parent's other path. Common
        // captures belong after the parent's condition, before entering the child.
        if (a.site.index, a.phase) <= (b.site.index, b.phase) {
            a
        } else {
            b
        }
    }

    fn clobbers(&self, body: &Body, location: Location, origin: Site, use_: Site) -> bool {
        let mut path = Vec::new();
        let mut scope = use_.region;
        while scope != origin.region {
            path.push(scope);
            scope = self.0[&scope]
                .parent
                .expect("a read is visible at its demand")
                .region;
        }
        let mut region = origin.region;
        let mut start = origin.index + 1;
        for child in path.into_iter().rev() {
            let parent = self.0[&child].parent.unwrap();
            if self.writes(body, location, region, start, parent.index) {
                return true;
            }
            region = child;
            start = 0;
        }
        self.writes(body, location, region, start, use_.index)
    }

    fn writes(
        &self,
        body: &Body,
        location: Location,
        region: usize,
        start: usize,
        end: usize,
    ) -> bool {
        let operations = &self.0[&region].region.operations[start..end];
        operations.iter().any(|operation| match operation {
            Operation::Store { location: other, .. } => location.may_overlap(*other, body),
            // A conditional write may clobber a later observation. Retain the
            // authored snapshot even when only some paths execute that write.
            Operation::If { branch, .. } => branch.walk().any(|region| {
                region.operations.iter().any(|operation| {
                    matches!(operation, Operation::Store { location: other, .. } if location.may_overlap(*other, body))
                })
            }),
            Operation::Load(_) => false,
        })
    }
}

#[derive(Clone, Copy)]
struct Demand {
    first: Point,
    at_first: bool,
    count: usize,
}

// A conversion within one Wasm type changes only the logical type. All its
// uses must reach the producer so sharing neither adds a local nor repeats work.
pub(super) fn representation(body: &Body, mut id: usize) -> usize {
    while let ValueKind::Convert(input) = body.values[id].kind {
        if wasm_type(body.values[id].ty) != wasm_type(body.values[input].ty) {
            break;
        }
        id = input;
    }
    id
}

fn demand(
    body: &Body,
    tree: &Tree<'_>,
    demands: &mut [Option<Demand>],
    value: usize,
    point: Point,
) {
    let value = representation(body, value);
    let entry = demands[value].get_or_insert(Demand {
        first: point,
        at_first: true,
        count: 0,
    });
    let first = tree.common(entry.first, point);
    entry.at_first = (first == entry.first && entry.at_first) || first == point;
    entry.first = first;
    entry.count += 1;
}

pub(super) fn plan(body: &Body) -> Placement {
    let tree = Tree::new(&body.region);
    let mut demands = vec![None; body.values.len()];
    for region in body.region.walk() {
        for (index, operation) in region.operations.iter().enumerate() {
            let point = Point::main(Site {
                region: region.id,
                index,
            });
            match operation {
                Operation::Store { location, value } => {
                    demand(body, &tree, &mut demands, location.base, point);
                    demand(body, &tree, &mut demands, *value, point);
                }
                Operation::If { condition, .. } => {
                    demand(body, &tree, &mut demands, *condition, point)
                }
                Operation::Load(_) => {}
            }
        }
        if let Some(terminal) = &region.terminal {
            for &value in terminal.inputs() {
                demand(
                    body,
                    &tree,
                    &mut demands,
                    value,
                    Point::main(Site {
                        region: region.id,
                        index: region.operations.len(),
                    }),
                );
            }
        }
    }
    let mut capture_points = vec![None; body.values.len()];
    // Operands precede consumers. A shared operation executes once at its chosen
    // point, so each input is needed once there, even if the result has later uses.
    for id in (0..body.values.len()).rev() {
        let Some(use_) = demands[id] else { continue };
        let mut anchor = use_.first;
        if let ValueKind::Load { location, site } = body.values[id].kind {
            if tree.clobbers(body, location, site, anchor.site) {
                anchor = Point::main(site);
            }
        }
        if (!use_.at_first || anchor != use_.first)
            && !matches!(
                body.values[id].kind,
                ValueKind::Constant(_) | ValueKind::Parameter(_)
            )
        {
            capture_points[id] = Some(anchor);
        }
        match body.values[id].kind {
            ValueKind::Binary(_, a, b) | ValueKind::Compare(_, a, b) => {
                demand(body, &tree, &mut demands, a, anchor);
                demand(body, &tree, &mut demands, b, anchor);
            }
            ValueKind::Normalize(input)
            | ValueKind::Convert(input)
            | ValueKind::Shift(_, input, _)
            | ValueKind::ZeroTest { input, .. } => demand(body, &tree, &mut demands, input, anchor),
            // Address reads preserve their snapshots where this read actually runs.
            ValueKind::Load { location, .. } => {
                demand(body, &tree, &mut demands, location.base, anchor)
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
            if demands[id].is_some_and(|use_| use_.count > 1 || capture_points[id].is_some())
                && !matches!(value.kind, ValueKind::Constant(_) | ValueKind::Parameter(_))
            {
                let slot = slot_types.len();
                slot_types.push(wasm_type(value.ty));
                Some(slot)
            } else {
                None
            }
        })
        .collect();
    let mut captures: HashMap<_, Vec<_>> = HashMap::new();
    // Increasing value order places captured dependencies before their users.
    for (id, point) in capture_points.into_iter().enumerate() {
        if let Some(point) = point {
            captures.entry(point.site).or_default().push(id);
        }
    }
    Placement {
        slots,
        captures,
        slot_types,
    }
}
