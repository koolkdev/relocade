//! Placement of reads and shared values among structured effects.
use std::collections::{BTreeMap, HashMap};

use wasm_encoder::ValType;

use crate::{
    control::{Region, Site},
    effects::Effects,
    emit::wasm_type,
    memory::Location,
    Body, Func, Operation, Terminal, ValueKind,
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
                for child in operation.children() {
                    pending.push((
                        child,
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

    fn common_bounds(&self, mut a: Point, mut b: Point) -> (Point, Point) {
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
        // captures belong after the parent's selector, before entering the child.
        if (a.site.index, a.phase) <= (b.site.index, b.phase) {
            (a, b)
        } else {
            (b, a)
        }
    }

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

    fn clobbers(
        &self,
        origin: Site,
        use_: Site,
        store: impl Fn(Location) -> bool,
        call: impl Fn(Func) -> bool,
    ) -> bool {
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
            if self.writes(region, start, parent.index, &store, &call) {
                return true;
            }
            region = child;
            start = 0;
        }
        self.writes(region, start, use_.index, &store, &call)
    }

    fn writes(
        &self,
        region: usize,
        start: usize,
        end: usize,
        store: &impl Fn(Location) -> bool,
        call: &impl Fn(Func) -> bool,
    ) -> bool {
        let direct = |operation: &Operation| match operation {
            Operation::Store { location, .. } => store(*location),
            Operation::Call { invocation, .. } => call(invocation.target),
            _ => false,
        };
        let operations = &self.0[&region].region.operations[start..end];
        operations.iter().any(|operation| {
            // Branch effects remain conservative, including returning arms whose
            // terminal call may write before leaving the function.
            direct(operation)
                || operation.children().any(|arm| {
                    arm.walk().any(|region| {
                        let operations_write = region.operations.iter().any(direct);
                        let terminal_writes = match &region.terminal {
                            Some(Terminal::TailCall(invocation)) => call(invocation.target),
                            _ => false,
                        };
                        operations_write || terminal_writes
                    })
                })
        })
    }
}

#[derive(Clone)]
struct Demand {
    first: Point,
    last: Point,
    at_first: bool,
    // Actual uses and physical captures, including repeated inputs at one point.
    // Lifted common bounds never replace these origins.
    points: Vec<Point>,
}

impl Demand {
    fn at(point: Point) -> Self {
        Self {
            first: point,
            last: point,
            at_first: true,
            points: vec![point],
        }
    }

    fn include(&mut self, point: Point, tree: &Tree<'_>) {
        let (first, _) = tree.common_bounds(self.first, point);
        let (_, last) = tree.common_bounds(self.last, point);
        self.at_first = (first == self.first && self.at_first) || first == point;
        self.first = first;
        self.last = last;
        self.points.push(point);
    }

    fn split_between_arms(&self, tree: &Tree<'_>) -> Option<Vec<Self>> {
        if self.first != self.last || self.first.phase != Phase::Header || self.at_first {
            return None;
        }
        // A common header is only a bound. Split actual uses by its direct child,
        // then choose normal placement within each child, including nested uses.
        // Physical demands in the parent's Main or Header phase cannot be split.
        let mut arms = BTreeMap::<usize, Self>::new();
        for &point in &self.points {
            let arm = tree.arm_containing(point, self.first.site)?;
            if let Some(demand) = arms.get_mut(&arm) {
                demand.include(point, tree);
            } else {
                arms.insert(arm, Self::at(point));
            }
        }
        Some(arms.into_values().collect())
    }
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
    if let Some(entry) = &mut demands[value] {
        entry.include(point, tree);
    } else {
        demands[value] = Some(Demand::at(point));
    }
}

fn parameter_only_values(body: &Body) -> Vec<bool> {
    let mut pure = Vec::with_capacity(body.values.len());
    // Every operand precedes its user. Loads, calls and joins retain their
    // existing snapshot and placement rules even when their inputs are pure.
    for value in &body.values {
        pure.push(match value.kind {
            ValueKind::Constant(_) | ValueKind::Parameter(_) => true,
            ValueKind::Binary(_, left, right)
            | ValueKind::Compare(_, left, right)
            | ValueKind::Shift {
                value: left,
                count: right,
                ..
            } => pure[left] && pure[right],
            ValueKind::Select {
                condition,
                when_true,
                when_false,
            } => pure[condition] && pure[when_true] && pure[when_false],
            ValueKind::Normalize(input)
            | ValueKind::Convert(input)
            | ValueKind::SignExtend(input)
            | ValueKind::Popcnt(input)
            | ValueKind::ZeroTest { input, .. } => pure[input],
            ValueKind::Load { .. }
            | ValueKind::CallResult { .. }
            | ValueKind::JoinResult { .. } => false,
        });
    }
    pure
}

pub(super) fn plan(body: &Body, effects: &[Effects]) -> Placement {
    let tree = Tree::new(&body.region);
    let parameter_only = parameter_only_values(body);
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
                Operation::If {
                    condition: selector,
                    ..
                }
                | Operation::Switch { selector, .. } => {
                    demand(body, &tree, &mut demands, *selector, point)
                }
                Operation::Call { invocation, output } => {
                    if effects[invocation.target.0].must_execute() {
                        demand(body, &tree, &mut demands, *output, point);
                    }
                }
                Operation::Load(_) => {}
            }
        }
        if let Some(terminal) = &region.terminal {
            if matches!(terminal, Terminal::Yield(_)) {
                continue;
            }
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
    let mut capture_points = vec![Vec::new(); body.values.len()];
    // Operands precede consumers. Each chosen placement needs its inputs once.
    // Pure expressions may have one placement in each mutually exclusive arm;
    // dependencies follow those physical points rather than a common header.
    for id in (0..body.values.len()).rev() {
        let Some(use_) = demands[id].clone() else {
            continue;
        };
        if let ValueKind::JoinResult { site } = body.values[id].kind {
            let operation = &tree.0[&site.region].region.operations[site.index];
            // The branch operation stays at its authored site. A live output needs
            // each yielding value only at the end of its own arm.
            for arm in operation.children() {
                if let Some(Terminal::Yield(value)) = arm.terminal {
                    demand(
                        body,
                        &tree,
                        &mut demands,
                        value,
                        Point::main(Site {
                            region: arm.id,
                            index: arm.operations.len(),
                        }),
                    );
                }
            }
            continue;
        }
        let uses = if parameter_only[id] {
            use_.split_between_arms(&tree)
        } else {
            None
        }
        .unwrap_or_else(|| vec![use_]);
        for use_ in uses {
            let mut anchor = use_.first;
            match body.values[id].kind {
                ValueKind::Load { location, site } => {
                    if tree.clobbers(
                        site,
                        anchor.site,
                        |other| location.may_overlap(other, body),
                        |target| effects[target.0].writes_location(location, body),
                    ) {
                        anchor = Point::main(site);
                    }
                }
                ValueKind::CallResult { site } => {
                    let summary = &effects[body.invocation(site).target.0];
                    if summary.must_execute() {
                        anchor = Point::main(site);
                    } else if let Effects::Known { reads, .. } = summary {
                        if tree.clobbers(
                            site,
                            anchor.site,
                            |location| {
                                reads
                                    .iter()
                                    .any(|read| read.overlaps_location(location, body))
                            },
                            |target| effects[target.0].writes_reads(reads),
                        ) {
                            anchor = Point::main(site);
                        }
                    }
                }
                _ => {}
            }
            if (!use_.at_first || anchor != use_.first)
                && !matches!(
                    body.values[id].kind,
                    ValueKind::Constant(_) | ValueKind::Parameter(_)
                )
            {
                capture_points[id].push(anchor);
            }
            match body.values[id].kind {
                ValueKind::Binary(_, a, b)
                | ValueKind::Compare(_, a, b)
                | ValueKind::Shift {
                    value: a, count: b, ..
                } => {
                    demand(body, &tree, &mut demands, a, anchor);
                    demand(body, &tree, &mut demands, b, anchor);
                }
                ValueKind::Select {
                    condition,
                    when_true,
                    when_false,
                } => {
                    demand(body, &tree, &mut demands, when_true, anchor);
                    demand(body, &tree, &mut demands, when_false, anchor);
                    demand(body, &tree, &mut demands, condition, anchor);
                }
                ValueKind::Normalize(input)
                | ValueKind::Convert(input)
                | ValueKind::SignExtend(input)
                | ValueKind::Popcnt(input)
                | ValueKind::ZeroTest { input, .. } => {
                    demand(body, &tree, &mut demands, input, anchor)
                }
                // Address reads preserve their snapshots where this read runs.
                ValueKind::Load { location, .. } => {
                    demand(body, &tree, &mut demands, location.base, anchor)
                }
                ValueKind::CallResult { site } => {
                    for &argument in &body.invocation(site).arguments {
                        demand(body, &tree, &mut demands, argument, anchor);
                    }
                }
                ValueKind::Constant(_) | ValueKind::Parameter(_) => {}
                ValueKind::JoinResult { .. } => unreachable!("join demands stay inside their arms"),
            }
        }
    }
    let mut slot_types = Vec::new();
    let slots = body
        .values
        .iter()
        .enumerate()
        .map(|(id, value)| {
            if demands[id].as_ref().is_some_and(|use_| {
                use_.points.len() > 1
                    || !capture_points[id].is_empty()
                    || matches!(value.kind, ValueKind::JoinResult { .. })
            }) && !matches!(value.kind, ValueKind::Constant(_) | ValueKind::Parameter(_))
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
    for (id, points) in capture_points.into_iter().enumerate() {
        for point in points {
            captures.entry(point.site).or_default().push(id);
        }
    }
    Placement {
        slots,
        captures,
        slot_types,
    }
}
