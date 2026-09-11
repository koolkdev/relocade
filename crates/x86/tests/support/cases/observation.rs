//! Batched, read-only observations of logical flags from copied CPU records.

use std::sync::OnceLock;
use wasm86_x86::CpuState;

use super::{expectations::check_flag_values, ExpectedFlags, FlagExpectation, Flags};
use crate::support::step::{Argument, Engine, Event, Input, Outcome, TestModule};

pub(in crate::support) struct FlagObservations {
    engine: Engine,
    records: Vec<CpuState>,
    checks: Vec<Check>,
}

struct Check {
    after: usize,
    before: Option<usize>,
    expected: Flags<FlagExpectation>,
    context: String,
}

impl FlagObservations {
    pub(in crate::support) fn new(engine: Engine) -> Self {
        Self {
            engine,
            records: Vec::new(),
            checks: Vec::new(),
        }
    }

    pub(in crate::support) fn initial(
        &mut self,
        cpu: CpuState,
        values: Flags<bool>,
        context: String,
    ) {
        let rule = |value| {
            if value {
                FlagExpectation::Set
            } else {
                FlagExpectation::Clear
            }
        };
        let expected = Flags {
            cf: rule(values.cf),
            pf: rule(values.pf),
            af: rule(values.af),
            zf: rule(values.zf),
            sf: rule(values.sf),
            of: rule(values.of),
        };
        self.after(
            cpu,
            cpu,
            ExpectedFlags::Logical {
                values: expected,
                preserve_record: false,
            },
            context,
        );
    }

    pub(in crate::support) fn after(
        &mut self,
        cpu: CpuState,
        prior: CpuState,
        flags: ExpectedFlags,
        context: String,
    ) {
        let ExpectedFlags::Logical {
            values: expected, ..
        } = flags
        else {
            return;
        };
        let before = expected
            .values()
            .iter()
            .any(|rule| matches!(rule, FlagExpectation::Preserved))
            .then(|| {
                let index = self.records.len();
                self.records.push(prior);
                index
            });
        let after = self.records.len();
        self.records.push(cpu);
        self.checks.push(Check {
            after,
            before,
            expected,
            context,
        });
    }

    pub(in crate::support) fn finish(self) {
        if self.records.is_empty() {
            return;
        }
        static OBSERVER: OnceLock<TestModule> = OnceLock::new();
        let module = OBSERVER.get_or_init(|| {
            TestModule::new(
                &crate::state::compile_flag_observer().expect("build the flag observer"),
            )
        });
        let mut input = Input::new(&self.records[0].to_bytes());
        input.cpu_patches_before_calls = self
            .records
            .iter()
            .map(|cpu| vec![(0, cpu.to_bytes().to_vec())])
            .collect();
        let observation = self.engine.observe(module, &input, self.records.len());
        assert!(observation.guest_unchanged && observation.machine_unchanged);
        assert_eq!(observation.events.len(), self.records.len());
        let values = self
            .records
            .iter()
            .zip(observation.events)
            .enumerate()
            .map(|(index, (cpu, event))| {
                let context = &self
                    .checks
                    .iter()
                    .find(|check| check.after == index || check.before == Some(index))
                    .unwrap()
                    .context;
                let Event::Return {
                    outcome: Outcome::Returned(values),
                    snapshot,
                } = event
                else {
                    panic!("{context}: the flag observer must return six bits");
                };
                assert_eq!(
                    snapshot.cpu,
                    cpu.to_bytes(),
                    "{context}: observing flags must preserve stored CPU bytes"
                );
                assert_eq!(snapshot.guest, None);
                let [cf, pf, af, zf, sf, of] = values.as_slice() else {
                    panic!("{context}: the flag observer must return six bits");
                };
                let bit = |value: &Argument| match value {
                    Argument::I32(0) => false,
                    Argument::I32(1) => true,
                    _ => panic!("{context}: non-Boolean flag observation {value:?}"),
                };
                Flags {
                    cf: bit(cf),
                    pf: bit(pf),
                    af: bit(af),
                    zf: bit(zf),
                    sf: bit(sf),
                    of: bit(of),
                }
            })
            .collect::<Vec<_>>();
        for check in self.checks {
            check_flag_values(
                check.before.map(|index| values[index]),
                check.expected,
                values[check.after],
                &check.context,
            );
        }
    }
}
