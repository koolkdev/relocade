# Contributing

This is a public repository.

- Commit messages use `component: title`.
- Commit implementation, permanent behavior tests and project documentation only.
  Keep temporary tools and development notes outside the repository.
- Prioritize generated guest execution speed in V8/TurboFan.
- Tests protect behavior, component invariants or external representations, with
  literal or independently derived expectations. Do not preserve obsolete internal APIs for tests.
- Keep components focused on current consumers; avoid speculative frameworks.
- Write comments in plain language to explain contracts and non-obvious reasoning.
- Review code, comments, tests and the staged diff before committing. Get an
  independent review for substantive changes.
- Present each prepared part for the user's ACK before committing it and starting
  the next substantial part.
