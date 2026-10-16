//! Detached branch topology and its publication into the parent body.
use super::JoinTarget;
use crate::{
    arena::Path,
    body::{Edge, Exit, Layout},
    function::PendingBlock,
    BlockBuilder, BuildError,
};

pub(super) enum Branches {
    Scope(PendingBlock),
    If {
        condition: usize,
        arms: [PendingBlock; 2],
    },
    Switch {
        selector: usize,
        cases: Vec<(u32, PendingBlock)>,
        default: PendingBlock,
    },
}

impl Branches {
    fn arms(&self) -> impl Iterator<Item = &PendingBlock> {
        let fixed = match self {
            Self::Scope(arm) => std::slice::from_ref(arm),
            Self::If { arms, .. } => arms,
            Self::Switch { .. } => &[],
        };
        let (cases, default) = match self {
            Self::Switch { cases, default, .. } => (cases.as_slice(), Some(default)),
            _ => (&[][..], None),
        };
        fixed
            .iter()
            .chain(cases.iter().map(|(_, arm)| arm))
            .chain(default)
    }

    fn continuing_path(&self, parent: &Path) -> Path {
        if let Self::Scope(arm) = self {
            return arm.path.clone();
        }
        let mut continuing = self.arms().filter(|arm| arm.falls_through());
        match (continuing.next(), continuing.next()) {
            (Some(arm), None) => arm.path.clone(),
            _ => parent.clone(),
        }
    }

    /// Publish only after every reachable arm has been built and checked.
    /// Ordinary arms reach the continuation only by falling through. Result
    /// arms can also yield earlier, before their final path decisions hold.
    pub(super) fn finish(
        self,
        body: &mut BlockBuilder<'_>,
        target: Option<JoinTarget>,
    ) -> Result<Vec<usize>, BuildError> {
        let (join, outputs, path) = if let Some(target) = target {
            let outputs = body
                .arena
                .complete_join(target.target, self.arms().map(|arm| arm.entry))?;
            (target.target, outputs, body.pending.path.clone())
        } else {
            (
                body.arena.block(body.pending.id, &[])?,
                Vec::new(),
                self.continuing_path(&body.pending.path),
            )
        };
        for arm in self.arms() {
            body.connect_fallthrough(arm, join)?;
        }
        let branch = body.pending.current;
        let edge = |arm: &PendingBlock| Edge {
            target: arm.entry,
            arguments: Vec::new(),
        };
        let (exit, layout) = match self {
            Self::Scope(arm) => (
                Exit::Jump(edge(&arm)),
                Layout::Scope {
                    preheader: branch,
                    body: arm.layout,
                    after: join,
                },
            ),
            Self::If {
                condition,
                arms: [taken, otherwise],
            } => (
                Exit::If {
                    condition,
                    taken: edge(&taken),
                    otherwise: edge(&otherwise),
                },
                Layout::If {
                    branch,
                    taken: taken.layout,
                    otherwise: otherwise.layout,
                    join,
                },
            ),
            Self::Switch {
                selector,
                cases,
                default,
            } => (
                Exit::Switch {
                    selector,
                    cases: cases.iter().map(|(key, arm)| (*key, edge(arm))).collect(),
                    default: edge(&default),
                },
                Layout::Switch {
                    branch,
                    cases: cases
                        .into_iter()
                        .map(|(key, arm)| (key, arm.layout))
                        .collect(),
                    default: default.layout,
                    join,
                },
            ),
        };
        body.arena.exit(branch, exit)?;
        body.pending.layout.push(layout);
        body.pending.current = join;
        body.pending.path = path;
        Ok(outputs)
    }
}
