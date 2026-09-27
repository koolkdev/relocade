//! Simplify uses with facts proved on their structured execution path.

mod facts;
mod sharing;
mod values;

use std::collections::HashMap;

use crate::body::{Block, Operation, Site, Target, Terminal, ValueTable};
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

    fn learn(&mut self, table: &ValueTable, condition: usize, truth: bool) {
        // Shared expressions may still refer to the original predicate even
        // after this branch has acquired an equivalent, simpler expression.
        self.facts.assume(table, condition, truth);
        if let Some(&rewritten) = self.rewritten.get(&condition) {
            self.facts.assume(table, rewritten, truth);
        }
        self.rewritten.clear();
    }

    fn share(&mut self, table: &mut ValueTable, values: &[usize]) {
        // A new group may be exclusive with an inherited group. Reconsider its
        // values using this anchor's facts, including cached dependent results.
        for value in values {
            self.shared.remove(value);
        }
        self.rewritten.clear();
        for &original in values {
            let mut rewritten = original;
            self.value(table, &mut rewritten);
            self.shared.insert(original, rewritten);
        }
    }

    fn arguments(&mut self, table: &mut ValueTable, arguments: &mut [usize]) {
        for argument in arguments {
            self.value(table, argument);
        }
    }
}

pub(super) fn simplify(table: &mut ValueTable, block: &mut Block) {
    let sharing = sharing::analyze(table, block);
    Simplifier { table, sharing }.visit(block, &mut Path::default());
}

struct Simplifier<'a> {
    table: &'a mut ValueTable,
    sharing: HashMap<Site, Vec<usize>>,
}

impl Simplifier<'_> {
    fn visit(&mut self, block: &mut Block, path: &mut Path) {
        let shared_tail = shared_tail(block);
        let operation_count = block.operations.len();
        for (index, operation) in block.operations.iter_mut().enumerate() {
            let site = Site {
                block: block.id,
                index,
            };
            if let Some(values) = self.sharing.get(&site) {
                path.share(self.table, values);
            }
            match operation {
                // Loads retain their authored addresses and snapshot identities. A
                // fact about an earlier read never describes a fresh read after a write.
                Operation::Nop | Operation::Load { .. } | Operation::Fence => {}
                Operation::Store { location, value } => {
                    path.value(self.table, &mut location.base);
                    path.value(self.table, value);
                }
                Operation::Atomic { access, .. } => {
                    *access = access.map(|mut value| {
                        path.value(self.table, &mut value);
                        value
                    });
                }
                Operation::Call { invocation, .. } => {
                    path.arguments(self.table, &mut invocation.arguments)
                }
                Operation::Block { block, .. } => self.visit(block, &mut path.fork()),
                Operation::Loop { initial, block, .. } => {
                    path.arguments(self.table, initial);
                    // Only inherited facts hold on every entry. Facts learned in an
                    // iteration do not escape the loop or flow around its backedges.
                    self.visit(block, &mut path.fork());
                }
                Operation::If {
                    condition,
                    branch,
                    else_branch,
                    ..
                } => {
                    let original = *condition;
                    path.value(self.table, condition);
                    let mut taken = path.fork();
                    taken.learn(self.table, original, true);
                    self.visit(branch, &mut taken);
                    let then_continues = continues(branch, site);
                    let else_continues = if let Some(other) = else_branch {
                        let mut skipped = path.fork();
                        skipped.learn(self.table, original, false);
                        self.visit(other, &mut skipped);
                        continues(other, site)
                    } else {
                        true
                    };
                    // The continuation can use the surviving arm's entry condition.
                    // Later facts within that arm might have been bypassed by an
                    // earlier yield, so they are deliberately not exported here.
                    if !then_continues && else_continues {
                        path.learn(self.table, original, false);
                    } else if then_continues && !else_continues {
                        path.learn(self.table, original, true);
                    }
                }
                Operation::BranchIf { condition, taken } => {
                    let original = *condition;
                    path.value(self.table, condition);
                    if shared_tail && index + 1 == operation_count {
                        // A br_if can carry one tuple for both edges. Simplify it
                        // before learning which edge is taken so it stays shared.
                        let Some(Terminal::Branch { arguments, .. }) = &mut taken.terminal else {
                            unreachable!("a shared tail has two branch edges");
                        };
                        path.arguments(self.table, arguments);
                        let Some(Terminal::Branch {
                            arguments: continued,
                            ..
                        }) = &mut block.terminal
                        else {
                            unreachable!("a shared tail has two branch edges");
                        };
                        continued.clone_from(arguments);
                        continue;
                    }
                    let mut edge = path.fork();
                    edge.learn(self.table, original, true);
                    self.visit(taken, &mut edge);
                    path.learn(self.table, original, false);
                }
                Operation::Switch {
                    selector,
                    cases,
                    default,
                    ..
                } => {
                    path.value(self.table, selector);
                    for case in cases {
                        self.visit(&mut case.block, &mut path.fork());
                    }
                    self.visit(default, &mut path.fork());
                }
            }
        }
        if let Some(values) = self.sharing.get(&Site {
            block: block.id,
            index: operation_count,
        }) {
            path.share(self.table, values);
        }
        if let Some(terminal) = &mut block.terminal {
            if shared_tail {
                return;
            }
            match terminal {
                Terminal::Trap => {}
                Terminal::Return(arguments) | Terminal::Branch { arguments, .. } => {
                    path.arguments(self.table, arguments)
                }
                Terminal::TailCall(invocation) => {
                    path.arguments(self.table, &mut invocation.arguments)
                }
            }
        }
    }
}

fn shared_tail(block: &Block) -> bool {
    let Some(Operation::BranchIf { taken, .. }) = block.operations.last() else {
        return false;
    };
    match (&taken.terminal, &block.terminal) {
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

fn continues(block: &Block, site: Site) -> bool {
    block.terminal.is_none() || block.exits_to(Target::exit(site)).next().is_some()
}
