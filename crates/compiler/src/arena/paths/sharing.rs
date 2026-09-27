//! Preserve calculations whose uses can execute together.
//!
//! An anchor chooses the facts used for a common rewrite. It does not move
//! evaluation or snapshot reads; placement still decides where values execute.

use std::collections::{HashMap, HashSet};

use super::super::ValueArena;
use crate::{
    control::{Block, BlockTree, Site, Target},
    Operation, Terminal, ValueDefinition,
};

pub(super) fn analyze(arena: &ValueArena, block: &Block) -> HashMap<Site, Vec<usize>> {
    let mut analysis = Analysis {
        arena,
        groups: Vec::new(),
        tree: BlockTree::new(block),
        continuations: HashMap::new(),
    };
    analysis.block(block, HashMap::new());
    let mut sites = HashMap::<Site, Vec<usize>>::new();
    for (index, group) in analysis.groups.iter().enumerate() {
        if group.parent == index && group.shared {
            sites.entry(group.anchor).or_default().push(group.value);
        }
    }
    for values in sites.values_mut() {
        values.sort_unstable();
        values.dedup();
    }
    sites
}

struct Analysis<'a> {
    arena: &'a ValueArena,
    groups: Vec<UseGroup>,
    tree: BlockTree<'a>,
    continuations: HashMap<Target, Live>,
}

// Different return paths can each reuse a value without sharing its rewrite.
// A use followed by either path joins their groups through that common use.
type Live = HashMap<usize, Vec<usize>>;

struct UseGroup {
    value: usize,
    anchor: Site,
    parent: usize,
    shared: bool,
}

impl Analysis<'_> {
    fn block(&mut self, block: &Block, after: Live) -> Live {
        let mut live = match &block.terminal {
            None => after,
            Some(Terminal::Branch { target, .. }) => {
                // Loop backedges start another iteration. Only uses that can
                // execute together within this iteration need a common rewrite.
                self.continuations.get(target).cloned().unwrap_or_default()
            }
            Some(_) => HashMap::new(),
        };
        if let Some(terminal) = &block.terminal {
            self.inputs(
                terminal.inputs().iter().copied(),
                Site {
                    block: block.id,
                    index: block.operations.len(),
                },
                &mut live,
            );
        }
        for (index, operation) in block.operations.iter().enumerate().rev() {
            let site = Site {
                block: block.id,
                index,
            };
            if operation.children().next().is_some() {
                let after = std::mem::take(&mut live);
                let target = Target::exit(site);
                self.continuations.insert(target, after.clone());
                if matches!(
                    operation,
                    Operation::If {
                        else_branch: None,
                        ..
                    } | Operation::BranchIf { .. }
                ) {
                    live.clone_from(&after);
                }
                for child in operation.children() {
                    for (value, groups) in self.block(child, after.clone()) {
                        let incoming = live.entry(value).or_default();
                        incoming.extend(groups);
                        for group in incoming.iter_mut() {
                            *group = self.root(*group);
                        }
                        incoming.sort_unstable();
                        incoming.dedup();
                    }
                }
                self.continuations.remove(&target);
            }
            match operation {
                Operation::Load(value) => {
                    let definition = self.arena.values[*value].definition;
                    self.inputs(definition.inputs(), site, &mut live);
                }
                Operation::Store { location, value } => {
                    self.inputs([location.base, *value], site, &mut live);
                }
                Operation::Atomic { access, .. } => self.inputs(access.inputs(), site, &mut live),
                Operation::Call { invocation, .. } => {
                    self.inputs(invocation.arguments.iter().copied(), site, &mut live);
                }
                Operation::Loop { initial, .. } => {
                    self.inputs(initial.iter().copied(), site, &mut live);
                }
                Operation::If { condition, .. } | Operation::BranchIf { condition, .. } => {
                    self.inputs([*condition], site, &mut live);
                }
                Operation::Switch { selector, .. } => self.inputs([*selector], site, &mut live),
                Operation::Nop | Operation::Fence | Operation::Block { .. } => {}
            }
        }
        live
    }

    fn inputs(&mut self, inputs: impl IntoIterator<Item = usize>, site: Site, live: &mut Live) {
        let mut pending: Vec<_> = inputs.into_iter().collect();
        let mut current = HashSet::new();
        while let Some(value) = pending.pop() {
            let ValueDefinition::Expression(expression) = self.arena.values[value].definition
            else {
                continue;
            };
            if !current.insert(value) {
                continue;
            }
            pending.extend(expression.inputs().copied());
        }
        for value in current {
            let groups = live.entry(value).or_default();
            let group = if let Some(&first) = groups.first() {
                let root = self.root(first);
                let mut anchor = self.common(site, self.groups[root].anchor);
                for &other in &groups[1..] {
                    let other = self.root(other);
                    if other != root {
                        anchor = self.common(anchor, self.groups[other].anchor);
                        self.groups[other].parent = root;
                    }
                }
                self.groups[root].anchor = anchor;
                self.groups[root].shared = true;
                root
            } else {
                let index = self.groups.len();
                self.groups.push(UseGroup {
                    value,
                    anchor: site,
                    parent: index,
                    shared: false,
                });
                index
            };
            groups.clear();
            groups.push(group);
        }
    }

    fn root(&mut self, mut group: usize) -> usize {
        while self.groups[group].parent != group {
            let parent = self.groups[group].parent;
            self.groups[group].parent = self.groups[parent].parent;
            group = parent;
        }
        group
    }

    fn common(&self, a: Site, b: Site) -> Site {
        let (a, b) = self.tree.common_block(a, b);
        Site {
            block: a.block,
            index: a.index.min(b.index),
        }
    }
}
