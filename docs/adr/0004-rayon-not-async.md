# 4. Use rayon for parallelism, not async

- Status: accepted
- Date: 2026-11-26

## Context

On a real project prunecss reads and scans thousands of content files and
parses one or more stylesheets. We want this to use every CPU core.

Rust offers two main models:

- **Async runtimes (Tokio and others)** are built for work that spends most
  of its time *waiting*, such as network servers handling many connections.
- **Data parallelism with rayon** splits CPU-heavy work across a thread pool
  with work stealing. Turning `iter()` into `par_iter()` is often the whole
  change.

prunecss's work is **CPU-bound**: reading local files is fast, and most of
the time goes into scanning bytes and parsing CSS. Tailwind's Oxide scanner,
which solves a very similar problem, uses rayon for the same reason.

Async also has costs: a runtime dependency, "coloured" functions that spread
`async` through the API, and harder debugging. A library that forces a
runtime on its users is also harder to embed in Node (via napi-rs) or in an
editor.

## Decision

- `prunecss-core` uses **rayon** for parallel work, and plain synchronous
  functions in its public API.
- `prunecss-core` does **not** depend on Tokio or any other async runtime,
  and has no `async fn` in its API.
- Async is allowed only in the future **language server** layer, which waits
  on messages from the editor. It will call the synchronous core from a
  blocking task or thread.

## Consequences

- The core stays simple to call from the CLI, from Node and from tests.
- Parallelism is easy to add and to remove for benchmarking.
- Each thread can build its own set of names and merge at the end, so no
  locks are needed in the hot path.
- Merged results must be put into a deterministic order before they reach
  the output, because thread scheduling order varies between runs.
- If prunecss ever needs to fetch remote content (for example, crawl a live
  site), that belongs in a front end, not in the core.
