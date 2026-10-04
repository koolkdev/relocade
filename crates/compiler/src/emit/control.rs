//! Lexical Wasm labels reference the graph's blocks and edge argument tuples.
use super::{Wasm, Writer};
use crate::body::{BlockId, Edge, Exit, Layout};
use wasm_encoder::BlockType;

impl Writer<'_> {
    pub(super) fn layouts(&mut self, layout: &[Layout], fallthrough: Option<BlockId>) {
        for (index, item) in layout.iter().enumerate() {
            let next = layout.get(index + 1).map(start).or(fallthrough);
            match item {
                Layout::Block(block) => {
                    if !self.reachable[block.0] {
                        continue;
                    }
                    self.block_items(*block);
                    self.exit(*block, next);
                }
                Layout::Scope {
                    preheader,
                    body,
                    after,
                } => {
                    if !self.reachable[preheader.0] {
                        continue;
                    }
                    self.block_items(*preheader);
                    self.emit(Wasm::Block(BlockType::Empty));
                    self.labels.push(Some(*after));
                    self.exit(*preheader, body.first().map(start).or(Some(*after)));
                    self.layouts(body, Some(*after));
                    self.labels.pop();
                    self.emit(Wasm::End);
                }
                Layout::If {
                    branch,
                    taken,
                    otherwise,
                    join,
                } => {
                    if !self.reachable[branch.0] {
                        continue;
                    }
                    self.block_items(*branch);
                    let Exit::If {
                        condition,
                        taken: taken_edge,
                        otherwise: other_edge,
                    } = &self.graph.blocks[branch.0].exit
                    else {
                        panic!("conditional layout references a conditional exit")
                    };
                    if let Some(index) = self.graph.blocks[branch.0]
                        .exit
                        .constant_edge_index(&self.graph.values)
                    {
                        let (edge, arm) = if index == 0 {
                            (taken_edge, taken)
                        } else {
                            (other_edge, otherwise)
                        };
                        self.emit(Wasm::Block(BlockType::Empty));
                        self.labels.push(Some(*join));
                        self.edge(edge, arm.first().map(start).or(Some(*join)));
                        self.layouts(arm, Some(*join));
                        self.labels.pop();
                        self.emit(Wasm::End);
                        continue;
                    }
                    let (condition, inverted) =
                        self.selection.condition(self.graph, *condition, false);
                    self.value(condition);
                    if inverted {
                        self.emit(Wasm::I32Eqz);
                    }
                    self.emit(Wasm::If(BlockType::Empty));
                    self.labels.push(Some(*join));
                    self.edge(taken_edge, taken.first().map(start).or(Some(*join)));
                    self.layouts(taken, Some(*join));
                    if !self.empty_arm(other_edge, otherwise, *join) {
                        self.emit(Wasm::Else);
                        self.edge(other_edge, otherwise.first().map(start).or(Some(*join)));
                        self.layouts(otherwise, Some(*join));
                    }
                    self.labels.pop();
                    self.emit(Wasm::End);
                }
                Layout::Loop {
                    preheader,
                    header,
                    body,
                    after,
                } => {
                    if !self.reachable[preheader.0] {
                        continue;
                    }
                    self.block_items(*preheader);
                    self.exit(*preheader, Some(*header));
                    self.emit(Wasm::Block(BlockType::Empty));
                    self.labels.push(Some(*after));
                    self.emit(Wasm::Loop(BlockType::Empty));
                    self.labels.push(Some(*header));
                    self.layouts(body, None);
                    self.labels.pop();
                    self.emit(Wasm::End);
                    self.labels.pop();
                    self.emit(Wasm::End);
                }
                Layout::Switch {
                    branch,
                    cases,
                    default,
                    join,
                } => {
                    if !self.reachable[branch.0] {
                        continue;
                    }
                    self.block_items(*branch);
                    let Exit::Switch {
                        selector,
                        cases: edges,
                        default: default_edge,
                    } = &self.graph.blocks[branch.0].exit
                    else {
                        panic!("switch layout references a switch exit")
                    };
                    if let Some(index) = self.graph.blocks[branch.0]
                        .exit
                        .constant_edge_index(&self.graph.values)
                    {
                        let (edge, arm) = if index < edges.len() {
                            (&edges[index].1, &cases[index].1)
                        } else {
                            (default_edge, default)
                        };
                        self.emit(Wasm::Block(BlockType::Empty));
                        self.labels.push(Some(*join));
                        self.edge(edge, arm.first().map(start).or(Some(*join)));
                        self.layouts(arm, Some(*join));
                        self.labels.pop();
                        self.emit(Wasm::End);
                        continue;
                    }
                    self.emit(Wasm::Block(BlockType::Empty));
                    self.labels.push(Some(*join));
                    for _ in 0..=cases.len() {
                        self.emit(Wasm::Block(BlockType::Empty));
                        self.labels.push(None);
                    }
                    self.value(*selector);
                    self.dispatch(&edges.iter().map(|(key, _)| *key).collect::<Vec<_>>());
                    for ((key, body), (edge_key, edge)) in cases.iter().zip(edges) {
                        assert_eq!(key, edge_key);
                        self.labels.pop();
                        self.emit(Wasm::End);
                        self.edge(edge, body.first().map(start).or(Some(*join)));
                        self.layouts(body, Some(*join));
                        self.emit(Wasm::Br(self.depth(*join)));
                    }
                    self.labels.pop();
                    self.emit(Wasm::End);
                    self.edge(default_edge, default.first().map(start).or(Some(*join)));
                    self.layouts(default, Some(*join));
                    self.labels.pop();
                    self.emit(Wasm::End);
                }
            }
        }
    }
    fn empty_arm(&self, edge: &Edge, layout: &[Layout], join: BlockId) -> bool {
        if !edge.arguments.is_empty() || edge.target != layout.first().map(start).unwrap_or(join) {
            return false;
        }
        for (index, item) in layout.iter().enumerate() {
            let Layout::Block(block) = item else {
                return false;
            };
            let body = &self.graph.blocks[block.0];
            if body.items.iter().any(|&item| self.selection.enabled(item)) {
                return false;
            }
            let Exit::Jump(edge) = &body.exit else {
                return false;
            };
            if !edge.arguments.is_empty()
                || edge.target != layout.get(index + 1).map(start).unwrap_or(join)
            {
                return false;
            }
        }
        true
    }
    fn depth(&self, target: BlockId) -> u32 {
        self.labels
            .iter()
            .rev()
            .position(|label| *label == Some(target))
            .expect("an edge targets an enclosing Wasm label") as u32
    }
    fn exit(&mut self, block: BlockId, fallthrough: Option<BlockId>) {
        match &self.graph.blocks[block.0].exit {
            Exit::Open => panic!("a reachable block has a completed exit"),
            Exit::Jump(edge) => self.edge(edge, fallthrough),
            Exit::If { .. } | Exit::Switch { .. } => {
                panic!("conditional exits use their lexical layout")
            }
            Exit::Return(values) => {
                for &value in values {
                    self.value(value);
                }
                self.emit(Wasm::Return);
            }
            Exit::TailCall { target, arguments } => {
                for &value in arguments {
                    self.value(value);
                }
                self.emit(Wasm::ReturnCall(
                    self.functions[target.0].expect("a tail-called function is retained"),
                ));
            }
            Exit::Trap => self.emit(Wasm::Unreachable),
        }
    }
    fn edge(&mut self, edge: &Edge, fallthrough: Option<BlockId>) {
        let parameters = &self.graph.blocks[edge.target.0].parameters;
        assert_eq!(parameters.len(), edge.arguments.len());
        // Evaluate the complete old tuple before overwriting any destination.
        // This also preserves parallel assignment for loop-carried swaps.
        for &argument in &edge.arguments {
            self.value(argument);
        }
        for &parameter in parameters.iter().rev() {
            self.emit(Wasm::LocalSet(self.local(parameter)));
        }
        if Some(edge.target) != fallthrough {
            self.emit(Wasm::Br(self.depth(edge.target)));
        }
    }
}
fn start(layout: &Layout) -> BlockId {
    match layout {
        Layout::Block(block) => *block,
        Layout::Scope { preheader, .. } | Layout::Loop { preheader, .. } => *preheader,
        Layout::If { branch, .. } | Layout::Switch { branch, .. } => *branch,
    }
}
