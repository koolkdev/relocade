//! Immutable full-carrier alias lists shared by path snapshots.
//! A refinement preserves earlier proofs; joins discard their cached answers.

#[derive(Default)]
pub(super) struct Equivalences {
    links: Vec<Link>,
}

struct Link {
    recipe: usize,
    previous: Option<usize>,
}

impl Equivalences {
    pub(super) fn recipes(&self, head: Option<usize>) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(head, |&id| self.links[id].previous).map(|id| self.links[id].recipe)
    }

    pub(super) fn record(&mut self, head: Option<usize>, recipe: usize) -> usize {
        let id = self.links.len();
        self.links.push(Link {
            recipe,
            previous: head,
        });
        id
    }
}
