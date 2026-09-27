//! Placement of reads and shared values among structured effects.
use std::collections::HashMap;

use wasm_encoder::ValType;

use crate::{
    body::{Block, BlockTree, Body, Operation, Site, Target, Terminal, ValueDefinition},
    effects::Effects,
    emit::wasm_type,
    memory::Location,
    Expression, Func,
};

mod calls;
mod order;
mod recompute;

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

struct Tree<'a>(&'a BlockTree<'a>);

impl Tree<'_> {
    fn common_bounds(&self, mut a: Point, mut b: Point) -> (Point, Point) {
        let (a_site, b_site) = self.0.common_block(a.site, b.site);
        for (point, site) in [(&mut a, a_site), (&mut b, b_site)] {
            if point.site.block != site.block {
                point.site = site;
                point.phase = Phase::Header;
            }
        }
        // A child cannot initialize values for its parent's other path. Common
        // captures belong after the parent's selector, before entering the child.
        if (a.site.index, a.phase) <= (b.site.index, b.phase) {
            (a, b)
        } else {
            (b, a)
        }
    }

    fn snapshot_anchor(
        &self,
        origin: Site,
        use_: Point,
        store: impl Fn(Location) -> bool,
        call: impl Fn(Func) -> bool,
    ) -> Point {
        if self.clobbers(origin, use_.site, store, call) {
            return Point::main(origin);
        }
        let mut anchor = use_;
        let mut block = use_.site.block;
        while block != origin.block {
            let parent = self
                .0
                .parent(block)
                .expect("a snapshot is visible at its demand");
            if matches!(
                self.0.block(parent.block).operations[parent.index],
                Operation::Loop { .. }
            ) {
                // Preserve an authored snapshot once before the first crossed
                // loop. Its enclosing guards and input scope remain intact.
                anchor = Point::main(parent);
            }
            block = parent.block;
        }
        anchor
    }

    fn clobbers(
        &self,
        origin: Site,
        use_: Site,
        store: impl Fn(Location) -> bool,
        call: impl Fn(Func) -> bool,
    ) -> bool {
        let mut path = Vec::new();
        let mut scope = use_.block;
        while scope != origin.block {
            path.push(scope);
            scope = self
                .0
                .parent(scope)
                .expect("a read is visible at its demand")
                .block;
        }
        let mut block = origin.block;
        let mut start = origin.index + 1;
        for child in path.into_iter().rev() {
            let parent = self.0.parent(child).unwrap();
            if self.writes_prefix(block, start, parent.index, &store, &call) {
                return true;
            }
            // An outer snapshot used in a loop must also survive writes after
            // that use: a backedge reaches the use again on the next iteration.
            if matches!(
                self.0.block(block).operations[parent.index],
                Operation::Loop { .. }
            ) && Self::block_may_write(self.0.block(child), &store, &call)
            {
                return true;
            }
            block = child;
            start = 0;
        }
        self.writes_prefix(block, start, use_.index, &store, &call)
    }

    fn writes_prefix(
        &self,
        block: usize,
        start: usize,
        end: usize,
        store: &impl Fn(Location) -> bool,
        call: &impl Fn(Func) -> bool,
    ) -> bool {
        // Stop before the demand's operation or terminal. Its child blocks
        // have not run yet and cannot clobber a selector's snapshot.
        self.0.block(block).operations[start..end]
            .iter()
            .any(|operation| {
                Self::writes_operation(operation, store, call)
                    || operation
                        .children()
                        .any(|child| Self::block_may_write(child, store, call))
            })
    }

    fn block_may_write(
        block: &Block,
        store: &impl Fn(Location) -> bool,
        call: &impl Fn(Func) -> bool,
    ) -> bool {
        // A whole loop lifetime includes every nested operation and terminal.
        // Returning tail calls can also write before exiting.
        block.walk().any(|block| {
            let operations_write = block
                .operations
                .iter()
                .any(|operation| Self::writes_operation(operation, store, call));
            let terminal_writes = match &block.terminal {
                Some(Terminal::TailCall(invocation)) => call(invocation.target),
                _ => false,
            };
            operations_write || terminal_writes
        })
    }

    fn writes_operation(
        operation: &Operation,
        store: &impl Fn(Location) -> bool,
        call: &impl Fn(Func) -> bool,
    ) -> bool {
        match operation {
            Operation::Store { location, .. } => store(*location),
            Operation::Call { invocation, .. } => call(invocation.target),
            Operation::Atomic { .. } | Operation::Fence => true,
            _ => false,
        }
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
}

// A conversion within one Wasm type changes only the logical type. All its
// uses must reach the producer so sharing neither adds a local nor repeats work.
pub(super) fn representation(body: &Body, mut id: usize) -> usize {
    while let ValueDefinition::Expression(Expression::Convert { input }) =
        body.values[id].definition
    {
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

struct Planner<'a> {
    body: &'a Body,
    effects: &'a [Effects],
    tree: Tree<'a>,
    demands: Vec<Option<Demand>>,
    capture_points: Vec<Vec<Point>>,
    saved: Vec<bool>,
    value_order: Vec<usize>,
}

pub(super) fn plan(body: &Body, effects: &[Effects], blocks: &BlockTree<'_>) -> Placement {
    let tree = Tree(blocks);
    let value_order = order::values(body, &tree);
    let mut planner = Planner {
        body,
        effects,
        tree,
        demands: vec![None; body.values.len()],
        capture_points: vec![Vec::new(); body.values.len()],
        saved: vec![false; body.values.len()],
        value_order,
    };
    planner.collect_demands();
    planner.place_values();
    planner.finish()
}

impl Planner<'_> {
    fn collect_demands(&mut self) {
        let body = self.body;
        let tree = &self.tree;
        let effects = self.effects;
        let demands = &mut self.demands;
        for block in body.block.walk() {
            for (index, operation) in block.operations.iter().enumerate() {
                let point = Point::main(Site {
                    block: block.id,
                    index,
                });
                match operation {
                    Operation::Loop {
                        initial, inputs, ..
                    } => {
                        // Keep every carried channel. Seeds and backedges are
                        // rooted before the reverse value walk, which otherwise
                        // assumes an acyclic expression graph.
                        for &value in initial {
                            demand(body, tree, demands, value, point);
                        }
                        for &input in inputs {
                            self.saved[input] = true;
                        }
                    }
                    Operation::Store { location, value } => {
                        demand(body, tree, demands, location.base, point);
                        demand(body, tree, demands, *value, point);
                    }
                    Operation::Atomic { access, .. } => {
                        for input in access.inputs() {
                            demand(body, tree, demands, input, point);
                        }
                    }
                    Operation::If {
                        condition: selector,
                        ..
                    }
                    | Operation::BranchIf {
                        condition: selector,
                        ..
                    }
                    | Operation::Switch { selector, .. } => {
                        demand(body, tree, demands, *selector, point)
                    }
                    Operation::Call {
                        invocation,
                        outputs,
                    } => {
                        if outputs.is_empty() && effects[invocation.target.0].must_execute() {
                            for &argument in &invocation.arguments {
                                demand(body, tree, demands, argument, point);
                            }
                        }
                    }
                    Operation::Nop
                    | Operation::Load { .. }
                    | Operation::Block { .. }
                    | Operation::Fence => {}
                }
            }
            if let Some(terminal) = &block.terminal {
                if matches!(terminal, Terminal::Branch { target, .. } if !target.entry) {
                    continue;
                }
                for &value in terminal.inputs() {
                    demand(
                        body,
                        tree,
                        demands,
                        value,
                        Point::main(Site {
                            block: block.id,
                            index: block.operations.len(),
                        }),
                    );
                }
            }
        }
    }

    fn place_values(&mut self) {
        let body = self.body;
        let effects = self.effects;
        // Visit consumers before their inputs, including rewritten edge arguments.
        // Recomputed expressions pass their actual placements to their inputs;
        // snapshot producers retain their own sharing and clobber rules.
        for index in (0..self.value_order.len()).rev() {
            let id = self.value_order[index];
            if matches!(
                body.values[id].definition,
                ValueDefinition::LoopInput { .. }
            ) {
                continue;
            }
            if let ValueDefinition::OperationResult { site, component } = body.values[id].definition
            {
                // Failed branch construction can leave values from a discarded block.
                let Some(operation) = self.tree.0.operation(site) else {
                    continue;
                };
                if matches!(operation, Operation::Atomic { .. }) {
                    // Ordered memory results are produced at the authored site.
                    // A live value must survive until its consumers demand it.
                    self.saved[id] = self.demands[id].is_some();
                    continue;
                }
                let (_, outputs) = self.tree.0.call(site);
                // Result groups follow every argument in the dependency order. Visit
                // the last component after all consumers have supplied demand.
                if component + 1 == outputs.len() {
                    self.place_call(site);
                }
                continue;
            }
            let tree = &self.tree;
            let demands = &mut self.demands;
            let capture_points = &mut self.capture_points;
            let saved = &mut self.saved;
            let Some(use_) = demands[id].clone() else {
                continue;
            };
            if let ValueDefinition::JoinResult { site, component } = body.values[id].definition {
                saved[id] = true;
                let operation = &tree.0.block(site.block).operations[site.index];
                // The branch operation stays at its authored site. A live output needs
                // each incoming component only at its actual branch site, including
                // exits nested within other control operations.
                for arm in operation.children() {
                    for (exit, arguments) in arm.exits_to(Target::exit(site)) {
                        demand(body, tree, demands, arguments[component], Point::main(exit));
                    }
                }
                continue;
            }
            for use_ in recompute::groups(body, id, use_, tree) {
                saved[id] |= use_.points.len() > 1;
                let mut anchor = use_.first;
                if let ValueDefinition::Load { site } = body.values[id].definition {
                    let location = tree.0.load_location(site);
                    anchor = tree.snapshot_anchor(
                        site,
                        anchor,
                        |other| location.may_overlap(other, body),
                        |target| effects[target.0].writes_location(location, body),
                    );
                    demand(body, tree, demands, location.base, anchor);
                }
                if (!use_.at_first || anchor != use_.first)
                    && !matches!(
                        body.values[id].definition,
                        ValueDefinition::Constant(_) | ValueDefinition::Parameter(_)
                    )
                {
                    capture_points[id].push(anchor);
                    saved[id] = true;
                }
                if let ValueDefinition::Expression(expression) = body.values[id].definition {
                    for &input in expression.inputs() {
                        demand(body, tree, demands, input, anchor);
                    }
                }
            }
        }
    }

    fn finish(self) -> Placement {
        let Self {
            body,
            capture_points,
            saved,
            value_order,
            ..
        } = self;
        let mut slot_types = Vec::new();
        let slots = body
            .values
            .iter()
            .enumerate()
            .map(|(id, value)| {
                if saved[id]
                    && !matches!(
                        value.definition,
                        ValueDefinition::Constant(_) | ValueDefinition::Parameter(_)
                    )
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
        // Captures at the same site must initialize dependencies before consumers.
        for id in value_order {
            for point in &capture_points[id] {
                captures.entry(point.site).or_default().push(id);
            }
        }
        Placement {
            slots,
            captures,
            slot_types,
        }
    }
}
