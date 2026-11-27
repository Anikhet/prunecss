# 1. Record architecture decisions

- Status: accepted
- Date: 2026-11-26

## Context

prunecss makes design choices that are not obvious from the code, such as
which CSS parser to use or how conservative matching should be. Future
contributors (and the maintainer, months later) need to know *why* a choice
was made before they change it. Commit messages and chat threads are hard to
find later.

## Decision

We record important decisions as short Architecture Decision Records (ADRs)
in `docs/adr/`.

- One decision per file, named `NNNN-short-title.md`, numbered in order.
- Each ADR has a status (proposed, accepted, superseded by NNNN), a date and
  three sections: **Context**, **Decision** and **Consequences**.
- Keep it to about one page. Plain language.
- ADRs are not edited after they are accepted, apart from fixing typos or
  the status. To change a decision, write a new ADR that supersedes the old
  one, and update the old one's status.
- A decision worth an ADR is one that is hard to reverse, affects the public
  API, or that a reasonable contributor might want to undo without knowing
  the reasons.

Larger proposals start as a GitHub issue. Once agreed, the outcome is written
up as an ADR in the same pull request as the code, when that makes sense.

## Consequences

- New contributors can read why things are the way they are.
- There is a small cost: writing a page for each big decision.
- [ARCHITECTURE.md](../../ARCHITECTURE.md) stays short and links here for the
  reasons behind its invariants.
