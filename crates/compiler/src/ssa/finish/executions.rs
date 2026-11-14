//! Keep execution histories dense even when refinement aliases leave sparse numbers.
use crate::body::BlockId;

pub(super) struct Executions {
    indices: Vec<u32>,
    histories: Vec<History>,
}

enum History {
    One((BlockId, usize)),
    Many(Vec<(BlockId, usize)>),
}

impl Executions {
    pub(super) fn new(values: usize) -> Self {
        Self {
            indices: vec![0; values],
            histories: Vec::new(),
        }
    }

    pub(super) fn extend(&mut self, values: usize) {
        self.indices.resize(values, 0);
    }

    pub(super) fn get(&self, number: usize) -> &[(BlockId, usize)] {
        let index = self.indices[number];
        if index == 0 {
            return &[];
        }
        match &self.histories[index as usize - 1] {
            History::One(execution) => std::slice::from_ref(execution),
            History::Many(executions) => executions,
        }
    }

    pub(super) fn record(&mut self, number: usize, block: BlockId, value: usize) {
        let execution = (block, value);
        let index = self.indices[number];
        if index == 0 {
            self.histories.push(History::One(execution));
            self.indices[number] =
                u32::try_from(self.histories.len()).expect("function execution histories fit u32");
            return;
        }
        let history = &mut self.histories[index as usize - 1];
        match history {
            History::One(previous) => *history = History::Many(vec![*previous, execution]),
            History::Many(executions) => executions.push(execution),
        }
    }
}
