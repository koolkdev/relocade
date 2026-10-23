//! Segment host calls belong to the instruction whose checkpoint follows them.

use crate::support::{
    blocks::BlockModules,
    execution::Frontend,
    machine::{expected, Image, Step},
    step::{Event, Input, Observation, SegmentQuery, SegmentResolution, TestModule},
};
use relocade_x86::ExecutionProfile;

pub(super) struct Scenario<'a> {
    profile: ExecutionProfile,
    code: &'a [u8],
    image: &'a Image,
    input: Input,
    steps: Vec<Step<'a>>,
    host_events: Vec<Vec<Event>>,
}

impl<'a> Scenario<'a> {
    pub(super) fn new(profile: ExecutionProfile, code: &'a [u8], image: &'a Image) -> Self {
        Self {
            profile,
            code,
            image,
            input: image.input(),
            steps: Vec::new(),
            host_events: Vec::new(),
        }
    }

    pub(super) fn step(self, step: Step<'a>) -> Self {
        self.instruction(step, Vec::new())
    }

    pub(super) fn resolve(mut self, replies: &[SegmentResolution], step: Step<'a>) -> Self {
        self.input.segment_resolutions.extend_from_slice(replies);
        let events = replies
            .iter()
            .map(|reply| Event::ResolveSegment {
                segment: reply.segment as i32,
                selector: i32::from(reply.selector),
            })
            .collect();
        self.instruction(step, events)
    }

    pub(super) fn query(mut self, replies: &[SegmentQuery], step: Step<'a>) -> Self {
        self.input.segment_queries.extend_from_slice(replies);
        let events = replies
            .iter()
            .map(|reply| Event::QuerySegmentDescriptor {
                selector: i32::from(reply.selector),
            })
            .collect();
        self.instruction(step, events)
    }

    fn instruction(mut self, step: Step<'a>, events: Vec<Event>) -> Self {
        self.steps.push(step);
        self.host_events.push(events);
        self
    }

    fn observation(&self, frontend: Frontend) -> Observation {
        assert!(!self.steps.is_empty(), "a scenario contains an instruction");
        // Compose all RAM patches before removing intermediate block snapshots.
        // Host calls stay in order on both frontends, including calls before faults.
        let mut wanted = expected(self.image, &self.steps);
        let mut boundaries = std::mem::take(&mut wanted.events).into_iter();
        for (index, calls) in self.host_events.iter().enumerate() {
            wanted.events.extend(calls.iter().cloned());
            for event in boundaries.by_ref() {
                let returned = matches!(event, Event::Return { .. });
                if matches!(frontend, Frontend::Interpreter) || index + 1 == self.steps.len() {
                    wanted.events.push(event);
                }
                if returned {
                    break;
                }
            }
        }
        wanted
    }

    #[track_caller]
    pub(super) fn check(&self) {
        let mut blocks = BlockModules::default();
        let block = blocks.get(
            &self.image.cpu,
            self.code,
            self.steps.len() as u32,
            self.profile,
        );
        self.check_with_block(block);
    }

    /// Reuse an entry or an explicitly larger compilation bound owned by the case.
    #[track_caller]
    pub(super) fn check_with_block(&self, block: &TestModule) {
        assert_eq!(
            block.observe(&self.input, 1),
            self.observation(Frontend::Block),
            "block {:?} {:02x?}",
            self.profile,
            self.code,
        );
        self.check_interpreter();
    }

    #[track_caller]
    pub(super) fn check_interpreter(&self) {
        assert_eq!(
            TestModule::interpreter_with_profile(self.profile)
                .observe(&self.input, self.steps.len()),
            self.observation(Frontend::Interpreter),
            "interpreter {:?} {:02x?}",
            self.profile,
            self.code,
        );
    }
}
