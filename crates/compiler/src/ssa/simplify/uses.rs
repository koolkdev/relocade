//! Direct uses of emitted definitions, stored without a vector per value.
#[derive(Default)]
pub(super) struct Uses {
    heads: Vec<u32>,
    edges: Vec<Edge>,
}

struct Edge {
    user: u32,
    next: u32,
}

impl Uses {
    pub(super) fn add(&mut self, input: usize, user: usize) {
        self.heads.resize(self.heads.len().max(input + 1), 0);
        self.edges.push(Edge {
            user: u32::try_from(user).expect("function values fit u32"),
            next: self.heads[input],
        });
        self.heads[input] = u32::try_from(self.edges.len()).expect("function uses fit u32");
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
