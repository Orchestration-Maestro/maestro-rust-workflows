# Domain docs

This repository has one context. Use its glossary and relevant architecture
decisions when exploring the codebase.

## Before exploring, read these

- [CONTEXT.md](../../CONTEXT.md) at the repository root defines the terms used here.
- `docs/adr/` holds architecture decision records (ADRs). Read those that touch
  the area you're about to work in.

If an optional document is absent, proceed silently. Don't suggest creating it
upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and
`/improve-codebase-architecture`) creates documents when terms or decisions
need recording.

## File structure

```text
/
├── CONTEXT.md
└── docs/
    └── adr/       (when architecture decisions need recording)
```

## Use the glossary's vocabulary

When naming a domain concept in an issue, proposal, hypothesis or test, use the
term defined in [CONTEXT.md](../../CONTEXT.md). Don't use a synonym its entry
explicitly avoids.

If a concept is missing, check whether the project uses it. Reconsider invented
language; note a real vocabulary gap for `/domain-modeling`.

## Flag ADR conflicts

If your output contradicts an existing ADR, say so rather than silently
overriding it:

> _Contradicts ADR-0007 (event-sourced orders), but worth reopening because…_
