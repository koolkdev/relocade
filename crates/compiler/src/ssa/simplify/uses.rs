//! Direct uses on the current dominator path, stored without a vector per value.
#[derive(Default)]
pub(super) struct Uses {
    heads: Vec<u32>,
    edges: Vec<Edge>,
}

struct Edge {
    input: u32,
    user: u32,
    next: u32,
}

impl Uses {
    pub(super) fn add(&mut self, input: usize, user: usize) {
        self.heads.resize(self.heads.len().max(input + 1), 0);
        self.edges.push(Edge {
            input: u32::try_from(input).expect("function values fit u32"),
            user: u32::try_from(user).expect("function values fit u32"),
            next: self.heads[input],
        });
        self.heads[input] = u32::try_from(self.edges.len()).expect("function uses fit u32");
    }

    pub(super) fn checkpoint(&self) -> usize {
        self.edges.len()
    }

    pub(super) fn leave(&mut self, checkpoint: usize) {
        while self.edges.len() > checkpoint {
            let edge = self.edges.pop().unwrap();
            self.heads[edge.input as usize] = edge.next;
        }
    }

    pub(super) fn of(&self, input: usize) -> impl Iterator<Item = usize> + '_ {
        let mut edge = self.heads.get(input).copied().unwrap_or(0);
        std::iter::from_fn(move || {
            if edge == 0 {
                return None;
            }
            let current = &self.edges[edge as usize - 1];
            edge = current.next;
            Some(current.user as usize)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Uses;

    #[test]
    fn leaving_a_scope_removes_its_users_and_restores_ancestor_users() {
        let mut uses = Uses::default();
        uses.add(1, 10);
        let parent = uses.checkpoint();
        uses.add(1, 11);
        uses.add(2, 12);
        let child = uses.checkpoint();
        uses.add(1, 13);
        uses.leave(child);
        assert_eq!(uses.of(1).collect::<Vec<_>>(), [11, 10]);
        assert_eq!(uses.of(2).collect::<Vec<_>>(), [12]);

        uses.leave(parent);
        assert_eq!(uses.of(1).collect::<Vec<_>>(), [10]);
        assert_eq!(uses.of(2).count(), 0);

        uses.add(2, 14);
        assert_eq!(uses.of(1).collect::<Vec<_>>(), [10]);
        assert_eq!(uses.of(2).collect::<Vec<_>>(), [14]);
    }
}
