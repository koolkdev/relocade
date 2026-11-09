//! Completion order must not affect retention, module indices or execution.

use crate::{fixture::signature, wasm::TestModule};
use futures_channel::oneshot;
use futures_util::FutureExt;
use relocade_compiler::{
    BuildError, CompileError, CompileExecutor, FunctionImport, MemoryImport, Program, TableImport,
    Type, I32, I64,
};
use std::{cell::RefCell, collections::VecDeque, pin::pin, rc::Rc};
use wasmparser::{Parser, Payload};

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(usize, Job)>,
    submitted: usize,
    completed: Vec<usize>,
    peak_pending: usize,
    fail: Option<usize>,
}

/// Deliberately local executor and futures; only the queued jobs must be Send.
#[derive(Default)]
struct ControlledExecutor(Rc<RefCell<Queue>>);

impl CompileExecutor for ControlledExecutor {
    type Error = &'static str;

    fn execute<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> impl std::future::Future<Output = Result<T, Self::Error>> {
        let (sender, receiver) = oneshot::channel();
        let mut queue = self.0.borrow_mut();
        let id = queue.submitted;
        queue.submitted += 1;
        let fail = queue.fail == Some(id);
        queue.jobs.push_back((
            id,
            Box::new(move || {
                let result = if fail {
                    Err("worker failed")
                } else {
                    Ok(job())
                };
                let _ = sender.send(result);
            }),
        ));
        queue.peak_pending = queue.peak_pending.max(queue.jobs.len());
        let local = Rc::clone(&self.0);
        async move {
            let result = receiver.await.expect("the controlled job completed");
            local.borrow_mut().completed.push(id);
            result
        }
    }
}

impl ControlledExecutor {
    fn compile(
        &self,
        program: Program,
        reverse: bool,
    ) -> Result<Vec<u8>, CompileError<&'static str>> {
        let mut compilation = pin!(program.compile_with(self));
        loop {
            if let Some(result) = compilation.as_mut().now_or_never() {
                return result;
            }
            let (_, job) = {
                let mut queue = self.0.borrow_mut();
                if reverse {
                    queue.jobs.pop_back()
                } else {
                    queue.jobs.pop_front()
                }
            }
            .expect("pending compilation has an executable job");
            job();
        }
    }
}

fn linked_functions(built: Rc<RefCell<Vec<&'static str>>>) -> Program {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "unused".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let imported = program.import_function(FunctionImport {
        module: "unused".into(),
        name: "function".into(),
        signature: signature(&[], &[]),
    });
    let table = program.import_table(TableImport {
        module: "unused".into(),
        name: "table".into(),
        minimum: 1,
        maximum: None,
        signature: signature(&[], &[Type::I32]),
    });
    let dead_built = built.clone();
    let dead = program.function::<fn() -> u32>(move |mut body, ()| {
        dead_built.borrow_mut().push("dead");
        body.store::<I32>(memory, 0, 99)?;
        body.call::<()>(imported, &[])?;
        body.tail_call_indirect(table, 0, &[])
    });
    let shared_built = built.clone();
    let shared = program.function::<fn() -> u32>(move |body, ()| {
        shared_built.borrow_mut().push("shared");
        body.return_(7)
    });
    let recursive = program.declare(signature(&[Type::I32], &[Type::I32]));
    program
        .define(recursive, move |mut body| {
            let remaining = body.parameter::<I32>(0)?;
            body.if_(remaining.eq(0), |arm| arm.return_(9))?;
            body.tail_call(recursive, &[remaining.sub(1).into()])
        })
        .unwrap();
    let run = program.function::<fn(bool) -> u32>(move |mut body, (condition,)| {
        built.borrow_mut().push("run");
        let nested = body
            .program()
            .function::<fn() -> u32>(move |body, ()| body.return_(5));
        body.if_(condition.eq(false), |arm| arm.return_(0))?;
        let result = body.if_value::<I32>(
            condition,
            |mut arm| {
                let a = shared.call(&mut arm, ())?;
                let b = nested.call(&mut arm, ())?;
                let c = arm.call::<I32>(recursive, &[3_u32.into()])?;
                arm.yield_(a.add(b).add(c))
            },
            |mut arm| {
                let value = dead.call(&mut arm, ())?;
                arm.yield_(value)
            },
        )?;
        body.return_(result)
    });
    let wide = program.function::<fn() -> u64>(move |mut body, ()| {
        let value = shared.call(&mut body, ())?;
        body.return_(value.unsigned().extend::<I64>().add(10_u64))
    });
    program.export("run", run).unwrap();
    program.export("wide", wide).unwrap();
    program
}

#[test]
fn reordered_jobs_preserve_retention_signatures_and_execution() {
    let serial = linked_functions(Rc::default()).compile().unwrap();
    for reverse in [false, true] {
        let built = Rc::new(RefCell::new(Vec::new()));
        let executor = ControlledExecutor::default();
        let bytes = executor
            .compile(linked_functions(built.clone()), reverse)
            .unwrap();
        assert_eq!(bytes, serial);
        let mut observed = built.borrow().clone();
        observed.sort_unstable();
        assert_eq!(observed, ["dead", "run", "shared"]);
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
        let mut instance = TestModule::new(&bytes).instantiate();
        assert_eq!(instance.call::<i32>(0), Ok(0));
        assert_eq!(instance.call::<i32>(1), Ok(21));
        assert_eq!(instance.call_export::<i64>("wide", ()), Ok(17));
        let queue = executor.0.borrow();
        assert!(
            queue.peak_pending >= 2,
            "independent jobs must be outstanding together"
        );
        if reverse {
            // Emission submits every retained body together. Its reversed final
            // completions must not change declaration order in the code section.
            assert!(queue.completed.iter().rev().take(3).is_sorted());
            assert!(queue.completed.windows(2).any(|pair| pair[0] > pair[1]));
        }
    }
}

fn one_function() -> Program {
    let mut program = Program::new();
    let function = program.function::<fn() -> u32>(move |body, ()| body.return_(42));
    program.export("run", function).unwrap();
    program
}

#[test]
fn execution_errors_distinguish_placement_and_emission_from_build_errors() {
    // One exported body schedules placement first and emission second.
    for fail in [0, 1] {
        let executor = ControlledExecutor::default();
        executor.0.borrow_mut().fail = Some(fail);
        assert!(matches!(
            executor.compile(one_function(), false),
            Err(CompileError::Execution("worker failed"))
        ));
        assert_eq!(executor.0.borrow().submitted, fail + 1);
    }
    let mut program = Program::new();
    let missing = program.declare(signature(&[], &[]));
    program.export("missing", missing).unwrap();
    let executor = ControlledExecutor::default();
    assert!(matches!(
        executor.compile(program, false),
        Err(CompileError::Build(BuildError::MissingBody))
    ));
    assert_eq!(executor.0.borrow().submitted, 0);
}

#[test]
fn an_empty_module_needs_no_executor_jobs() {
    let executor = ControlledExecutor::default();
    assert_eq!(
        executor.compile(Program::new(), false).unwrap(),
        Program::new().compile().unwrap()
    );
    assert_eq!(executor.0.borrow().submitted, 0);
}
