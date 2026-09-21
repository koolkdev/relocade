# Contributing

This is a public repository. Commit messages use `component: title`.

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

## Review and validation

- Before calling a part ready, review the expanded modules, not only the added lines.
  Check for missed generalization, duplicated mechanisms, awkward APIs, misleading
  names, stale comments and tests that preserve obsolete implementation details.
- Tests protect behavior, component invariants or external representations, using
  literal or independently derived expectations. Keep test names and files aligned
  with the behavior and owner they protect.
- Use the ordinary Wasmtime suite for broad correctness coverage. Use V8/TurboFan
  for performance measurements and focused correctness checks of changed generated
  code before timing. Prefer focused block tests while iterating on an instruction.
  Full V8 runs are also appropriate for routine broad verification; choose coverage
  to match the change and measured test cost.
- Compare Wasm bytes first; do not benchmark identical output. Measure changed
  output with meaningful workloads and matching execution boundaries, and report
  uncertainty honestly.
- Preserve the measured memory-backed handling of mixed-width register aliases.
  Earlier V8 measurements favored backing reads and writes over extract/merge
  expressions; textbook SSA shape alone is not a reason to replace that design.
  Require relevant runtime evidence before changing the mechanism.
- Focus performance work on frequent hot paths. Keep rare paths correct and watch
  for material regressions, but do not chase possible 1–5% gains there.
- For extremely rare opcodes, skip reference comparisons and performance measurements;
  retain architectural correctness tests and relevant engine coverage.
- When comparing instruction additions, use common generated snapshot blocks. Defer
  interpreter performance comparisons until interpreter work, and keep split-page
  accesses in correctness coverage rather than instruction performance workloads.
- Byte preservation is evidence, not a reason to retain a poor abstraction or duplicate
  a mechanism. Review design changes together with their generated-code consequences.
- Complete implementation, comments, tests and relevant validation for the current
  part before presenting it. Get an independent review for substantive changes and
  inspect the exact staged diff, including every file proposed for the commit.
- Report decisions, verification and material limitations, including module-boundary
  decisions for substantially expanded files.
- Every completed change report includes a concise walkthrough of added or reshaped
  types, schemas and consumer APIs, with representative code excerpts and the reasons
  for their design. Distinguish new interfaces from existing ones reused by the change.
  If these did not change, say so.
- Split file accounting into two tables: production code and documentation, and tests
  and fixtures, including test modules under src. Each changed file appears exactly
  once with a clickable filename, added lines, removed lines and reason for the change.
- Wait for the user's fresh ACK before committing the prepared part and starting the
  next substantial part. Resolve routine implementation choices while completing the
  current part; do not leave it half-finished merely to request those choices.
