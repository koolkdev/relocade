//! Simplify uses with facts proved on their structured execution path.

mod facts;
mod sharing;
mod values;

use std::collections::HashMap;

use super::ValueArena;
use crate::{
    control::{Region, Site, Target},
    memory::AtomicKind,
    Operation, Terminal,
};
use facts::Facts;

#[derive(Default)]
struct Path {
    facts: Facts,
    // Learning a new fact invalidates this path's cached simplifications.
    rewritten: HashMap<usize, usize>,
    // Rewrites shared by uses that can execute together.
    shared: HashMap<usize, usize>,
}

impl Path {
    fn fork(&self) -> Self {
        Self {
            facts: self.facts.clone(),
            rewritten: self.rewritten.clone(),
            shared: self.shared.clone(),
        }
    }

    fn learn(&mut self, arena: &ValueArena, condition: usize, truth: bool) {
        // Shared expressions may still refer to the original predicate even
        // after this branch has acquired an equivalent, simpler expression.
        self.facts.assume(arena, condition, truth);
        if let Some(&rewritten) = self.rewritten.get(&condition) {
            self.facts.assume(arena, rewritten, truth);
        }
        self.rewritten.clear();
    }

    fn share(&mut self, arena: &mut ValueArena, values: &[usize]) {
        // A new group may be exclusive with an inherited group. Reconsider its
        // values using this anchor's facts, including cached dependent results.
        for value in values {
            self.shared.remove(value);
        }
        self.rewritten.clear();
        for &original in values {
            let mut rewritten = original;
            self.value(arena, &mut rewritten);
            self.shared.insert(original, rewritten);
        }
    }

    fn arguments(&mut self, arena: &mut ValueArena, arguments: &mut [usize]) {
        for argument in arguments {
            self.value(arena, argument);
        }
    }
}

pub(super) fn simplify(arena: &mut ValueArena, region: &mut Region) {
    let sharing = sharing::analyze(arena, region);
    Simplifier { arena, sharing }.visit(region, &mut Path::default());
}

struct Simplifier<'a> {
    arena: &'a mut ValueArena,
    sharing: HashMap<Site, Vec<usize>>,
}

impl Simplifier<'_> {
    fn visit(&mut self, region: &mut Region, path: &mut Path) {
        let shared_tail = shared_tail(region);
        let operation_count = region.operations.len();
        for (index, operation) in region.operations.iter_mut().enumerate() {
            let site = Site {
                region: region.id,
                index,
            };
            if let Some(values) = self.sharing.get(&site) {
                path.share(self.arena, values);
            }
            match operation {
                // Loads retain their authored addresses and snapshot identities. A
                // fact about an earlier read never describes a fresh read after a write.
                Operation::Nop | Operation::Load(_) | Operation::Fence => {}
                Operation::Store { location, value } => {
                    path.value(self.arena, &mut location.base);
                    path.value(self.arena, value);
                }
                Operation::Atomic { access, .. } => {
                    path.value(self.arena, &mut access.location.base);
                    match &mut access.operation {
                        AtomicKind::Load => {}
                        AtomicKind::Store { value }
                        | AtomicKind::Add(value)
                        | AtomicKind::Subtract(value)
                        | AtomicKind::And(value)
                        | AtomicKind::Or(value)
                        | AtomicKind::Xor(value)
                        | AtomicKind::Exchange(value) => path.value(self.arena, value),
                        AtomicKind::CompareExchange {
                            expected,
                            replacement,
                        } => {
                            path.value(self.arena, expected);
                            path.value(self.arena, replacement);
                        }
                    }
                }
                Operation::Call { invocation, .. } => {
                    path.arguments(self.arena, &mut invocation.arguments)
                }
                Operation::Block { region, .. } => self.visit(region, &mut path.fork()),
                Operation::Loop {
                    initial, region, ..
                } => {
                    path.arguments(self.arena, initial);
                    // Only inherited facts hold on every entry. Facts learned in an
                    // iteration do not escape the loop or flow around its backedges.
                    self.visit(region, &mut path.fork());
                }
                Operation::If {
                    condition,
                    branch,
                    else_branch,
                    ..
                } => {
                    let original = *condition;
                    path.value(self.arena, condition);
                    let mut taken = path.fork();
                    taken.learn(self.arena, original, true);
                    self.visit(branch, &mut taken);
                    let then_continues = continues(branch, site);
                    let else_continues = if let Some(other) = else_branch {
                        let mut skipped = path.fork();
                        skipped.learn(self.arena, original, false);
                        self.visit(other, &mut skipped);
                        continues(other, site)
                    } else {
                        true
                    };
                    // The continuation can use the surviving arm's entry condition.
                    // Later facts within that arm might have been bypassed by an
                    // earlier yield, so they are deliberately not exported here.
                    if !then_continues && else_continues {
                        path.learn(self.arena, original, false);
                    } else if then_continues && !else_continues {
                        path.learn(self.arena, original, true);
                    }
                }
                Operation::BranchIf { condition, taken } => {
                    let original = *condition;
                    path.value(self.arena, condition);
                    if shared_tail && index + 1 == operation_count {
                        // A br_if can carry one tuple for both edges. Simplify it
                        // before learning which edge is taken so it stays shared.
                        let Some(Terminal::Branch { arguments, .. }) = &mut taken.terminal else {
                            unreachable!("a shared tail has two branch edges");
                        };
                        path.arguments(self.arena, arguments);
                        let Some(Terminal::Branch {
                            arguments: continued,
                            ..
                        }) = &mut region.terminal
                        else {
                            unreachable!("a shared tail has two branch edges");
                        };
                        continued.clone_from(arguments);
                        continue;
                    }
                    let mut edge = path.fork();
                    edge.learn(self.arena, original, true);
                    self.visit(taken, &mut edge);
                    path.learn(self.arena, original, false);
                }
                Operation::Switch {
                    selector,
                    cases,
                    default,
                    ..
                } => {
                    path.value(self.arena, selector);
                    for case in cases {
                        self.visit(&mut case.region, &mut path.fork());
                    }
                    self.visit(default, &mut path.fork());
                }
            }
        }
        if let Some(values) = self.sharing.get(&Site {
            region: region.id,
            index: operation_count,
        }) {
            path.share(self.arena, values);
        }
        if let Some(terminal) = &mut region.terminal {
            if shared_tail {
                return;
            }
            match terminal {
                Terminal::Trap => {}
                Terminal::Return(arguments) | Terminal::Branch { arguments, .. } => {
                    path.arguments(self.arena, arguments)
                }
                Terminal::TailCall(invocation) => {
                    path.arguments(self.arena, &mut invocation.arguments)
                }
            }
        }
    }
}

fn shared_tail(region: &Region) -> bool {
    let Some(Operation::BranchIf { taken, .. }) = region.operations.last() else {
        return false;
    };
    match (&taken.terminal, &region.terminal) {
        (
            Some(Terminal::Branch { arguments, .. }),
            Some(Terminal::Branch {
                arguments: continued,
                ..
            }),
        ) => taken.operations.is_empty() && arguments == continued,
        _ => false,
    }
}

fn continues(region: &Region, site: Site) -> bool {
    region.terminal.is_none() || region.exits_to(Target::exit(site)).next().is_some()
}
