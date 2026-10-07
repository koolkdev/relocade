//! Resolve constant exits and keep lexical nesting consistent with live edges.
use super::super::{BlockId, Exit, FunctionGraph, Layout};

#[cfg(test)]
mod tests;

pub(super) fn simplify(graph: &mut FunctionGraph, reachable: &[bool]) {
    let layout = std::mem::take(&mut graph.layout);
    graph.layout = Control::new(graph, reachable).simplify(layout);
    for block in &mut graph.blocks {
        if let Some(index) = block.exit.constant_edge_index(&graph.values) {
            block.exit = Exit::Jump(block.exit.edge(index).clone());
        }
    }
}

struct Control<'a> {
    graph: &'a FunctionGraph,
    reachable: &'a [bool],
    incoming: Vec<Vec<BlockId>>,
}

impl<'a> Control<'a> {
    fn new(graph: &'a FunctionGraph, reachable: &'a [bool]) -> Self {
        let mut incoming = vec![Vec::new(); graph.blocks.len()];
        for (source, &live) in reachable.iter().enumerate() {
            if live {
                for edge in graph.outgoing(BlockId(source)) {
                    incoming[edge.target.0].push(BlockId(source));
                }
            }
        }
        Self {
            graph,
            reachable,
            incoming,
        }
    }

    fn simplify(&self, layout: Vec<Layout>) -> Vec<Layout> {
        let mut result = Vec::with_capacity(layout.len());
        for item in layout {
            if !self.reachable[item.entry().0] {
                continue;
            }
            match item {
                Layout::Block(_) => result.push(item),
                Layout::Scope {
                    preheader,
                    body,
                    after,
                } => self.scope(&mut result, preheader, body, after),
                Layout::If {
                    branch,
                    taken,
                    otherwise,
                    join,
                } => match self.graph.blocks[branch.0]
                    .exit
                    .constant_edge_index(&self.graph.values)
                {
                    Some(index) => {
                        let body = if index == 0 { taken } else { otherwise };
                        self.scope(&mut result, branch, body, join);
                    }
                    None => result.push(Layout::If {
                        branch,
                        taken: self.simplify(taken),
                        otherwise: self.simplify(otherwise),
                        join,
                    }),
                },
                Layout::Switch {
                    branch,
                    cases,
                    default,
                    join,
                } => match self.graph.blocks[branch.0]
                    .exit
                    .constant_edge_index(&self.graph.values)
                {
                    Some(index) => {
                        let body = cases
                            .into_iter()
                            .nth(index)
                            .map(|(_, body)| body)
                            .unwrap_or(default);
                        self.scope(&mut result, branch, body, join);
                    }
                    None => result.push(Layout::Switch {
                        branch,
                        cases: cases
                            .into_iter()
                            .map(|(key, body)| (key, self.simplify(body)))
                            .collect(),
                        default: self.simplify(default),
                        join,
                    }),
                },
                Layout::Loop {
                    preheader,
                    header,
                    body,
                    after,
                } => result.push(Layout::Loop {
                    preheader,
                    header,
                    body: self.simplify(body),
                    after,
                }),
            }
        }
        result
    }

    fn scope(
        &self,
        result: &mut Vec<Layout>,
        preheader: BlockId,
        body: Vec<Layout>,
        after: BlockId,
    ) {
        let body = self.simplify(body);
        let fallthrough = match body.last() {
            None => Some(preheader),
            Some(Layout::Block(block)) => Some(*block),
            _ => None,
        };
        // Early outward branches still need a label. A sole final entrance can
        // fall through directly, including through nested resolved decisions.
        if self.incoming[after.0]
            .iter()
            .any(|&source| Some(source) != fallthrough)
        {
            result.push(Layout::Scope {
                preheader,
                body,
                after,
            });
        } else {
            result.push(Layout::Block(preheader));
            result.extend(body);
        }
    }
}
