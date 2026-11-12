# Contributing

This is a public repository. Commit messages use `component: title`.

## Compatibility target

- Aim for Pentium 4 (P4) user-mode instruction-set parity. Prioritize missing P4
  instructions and architectural behavior when recommending or choosing work.
  Check instruction availability as well as semantics against Intel's manuals.
  Existing support for later extensions does not expand this target.

## Clean product code

- Commit implementation, permanent behavior tests and useful project documentation only.
  Never commit migration notes, migration scripts, temporary generators, comparison
  ledgers or development-process artifacts. Keep those outside the repository.
- Rebuild deliberately. Intel's architecture manuals are the authority for
  architectural behavior. Reference code supplies comparison and performance
  evidence; it can contain defects and is not a correctness oracle or a design
  template to copy without review.
- Review comments, names, test names and test-file placement with the same care as
  implementation. Comments explain contracts and non-obvious reasons in plain
  language; rewrite unclear or obsolete comments instead of carrying them forward.

## Shared mechanisms and ownership

- Bound a part by a coherent capability, not by refusing to improve shared modules.
  Every new consumer should prompt a review of the abstraction it extends.
- Generalize mechanisms when their common responsibility becomes clear. Expose
  varying inputs explicitly, update existing consumers to the common mechanism,
  and remove superseded special cases in the same part. Do not accumulate one-off
  variants, wrappers or duplicated algorithms for each new instruction or access.
- Keep semantic operations distinct from implementation shortcuts. A constant and
  a computed operand can use the same expression model; constant specialization
  belongs in folding or lowering rather than a duplicate operation family.
- Put behavior in its owner: instruction definitions describe forms, decoding reads
  bytes, shared semantics define effects, memory owns access policy and faults,
  state owns architectural layout and publication, and the compiler owns value
  construction, placement and lowering.
- Trust internal consistency, including valid physical backing for present memory
  mappings. Unexpected host or Wasm traps from broken internal invariants are bugs,
  not supported execution outcomes. Do not add APIs, scheduling constraints,
  recovery paths or tests to preserve their occurrence, ordering or post-trap state.
  Keep expected guest faults and deliberately authored compiler traps distinct
  from these internal failures.
- Keep representation choices and invariants with their owner. Interfaces should
  express the caller's intent without requiring knowledge of internal details.
  When callers must coordinate internal steps or repeat special cases, review
  whether responsibility belongs on the other side of the boundary.
- Review execution policies across interpreter and JIT. Distinguish compilation
  bounds, runtime stopping guarantees and host responsiveness; behavior in one
  reference frontend does not settle the shared contract.
- Optimize the JIT for common workloads. Move rare cases to the interpreter when
  doing so materially improves common-path speed or substantially reduces generated
  code size, even if the JIT already supports those cases. Keep common cases compiled.
  Justify rarity from expected or measured workloads, measure the speed or code-size
  benefit, and test precise restart and interpreter correctness for excluded cases.
- Keep mixed-width register aliases memory-backed unless runtime measurements
  justify a change; earlier V8 measurements favored this over extract/merge expressions.
- Use `guard` when a failed condition should restart the current instruction
  in the interpreter. Build paths requiring fallback through `if_can_fallback`,
  or within an execution owner that already requires that capability. Guard before
  current-instruction effects and apply refinements only on the continuing path.
  Use runtime branching when both outcomes belong in the JIT, such as choosing
  between fast and general calculations for common inputs. If compile-time facts
  already determine the calculation, select it at compile time.
- Treat growing argument lists and repeated context forwarding as an ownership
  problem. Give the responsible builder or reader the operations and lifecycle it
  manages; do not merely move loose parameters into an inert context structure.
- Pure value selection and effectful branch execution have different contracts.
  Use value expressions for calculations that placement should schedule. Investigate
  missing compiler operations or placement defects before making consumers manage
  evaluation placement by hand.
- Readability is a design requirement. Deeply nested builder closures, opaque tuples
  and repeated dispatch plumbing should trigger a structural review. Prefer named
  fields and focused mechanisms whose control flow follows the policy being expressed.
- Review file and module boundaries before extending a substantial owner, after
  adding responsibilities, and before calling a part ready. Identify independently
  growing responsibilities and split them into cohesive modules during that part;
  do not defer an evident boundary until another feature or a user points it out.
- Use file size as a review signal, not a splitting threshold. Keep tightly coupled
  code together and give each extracted module a clear responsibility and a small
  interface. Keep visibility limited to the owners that need it; do not introduce
  traits, forwarding wrappers or shared context structures solely to move code.
  Apply the same review to tests, keeping related fixtures and assertions together.
- Use terminology that explains the domain. Distinguish instruction encoding from
  snapshot or runtime decoding, and storage locations from immediate values.
- Name values for their contents or role, and functions for the behavior they perform.
  Avoid vague labels such as `selected` without a clear noun. A name should not
  require tracing its uses to discover what it represents. Review local variables,
  predicates, helper handles and tests as carefully as public API names.

## Consumer APIs

- Keep logical types, storage widths and Wasm carriers distinct. Function signatures
  should express logical types, including one-bit results, without leaking backend
  representation choices unnecessarily.
- Keep value operations fluent, such as `value.add(1)`. The compiler interprets native
  literals at its input boundary; consumers should not need explicit constant-value
  construction for ordinary operands. Retain a symbolic value when it is actually needed.
- Keep body construction and completion clear. Avoid overlapping completion methods
  or boilerplate that consumers must repeat because an owning module is too narrow.
  Common construction paths should not expose setup steps needed only for forward
  references or other advanced cases.

## Tests

- Add or extend permanent tests for a distinct observable behavior, component
  invariant or external representation. Before adding coverage, inspect the owning
  suite and identify the failure the new case would detect that existing cases
  would miss. A code change does not automatically require a new test or test file.
- Keep comprehensive mechanism coverage with its owner. Consumer tests should
  cover independently wired forms and interactions that can fail separately.
  A new wrapper, type name or caller does not by itself justify repeating the
  mechanism's matrix. Keep test names and files aligned with their current owner.
- Integration tests should exercise the production entrypoint or wiring they
  claim to protect. Use representative cases; multiply addressing, prefix, width,
  alias, flag or backend dimensions only when their interaction adds a failure
  mode. Preserve distinct guest faults, instruction forms, interpreter/JIT and
  native/browser behavior, ABI contracts and compile-fail guarantees.
- Use literal or independently derived expectations. Assert the contract rather
  than incidental IDs, ordering or generated instructions. Keep code-shape checks
  where they protect an explicit compiler optimization or representation invariant.
  Each assertion must observe something its fixture can actually distinguish.
- When consolidating or removing cases, identify the surviving coverage and retain
  any unique interaction in a focused replacement. Keep helpers with their owner;
  they should adapt inputs and observe outputs without reproducing production
  behavior. Prefer extending a coherent existing fixture over adding another layer.

## Review and validation

- Finish the requested change, commit, push and open or update the PR.
- Let CI run builds, tests, formatting and static checks. Run local checks only
  when needed to implement or debug the change, or when the user asks.
- Do not add extra reviews, involve another reviewer or wait for CI unless asked.
- Report the result briefly. Distinguish checks actually run from verification
  left to CI.
