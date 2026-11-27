# 3. Conservative "bag of names" matching

- Status: accepted
- Date: 2026-11-26

## Context

To decide whether a selector is used, a CSS remover can take one of two
approaches.

- **Token bag (PurgeCSS).** Pull a flat set of words out of each content file
  without running or rendering it. Keep a selector if every name it needs
  appears somewhere in the set. Structure is ignored, so `.a > .b` is kept
  whenever `a` and `b` both appear anywhere. This works on templates, JSX and
  any text, and it errs towards keeping too much.
- **DOM matching (DropCSS, UnCSS).** Build a real document tree from rendered
  HTML (UnCSS even runs page scripts in jsdom) and test each selector against
  it. This understands combinators, `:nth-child()` and `:not()`, so it removes
  more. But it needs fully rendered, well-formed HTML, it cannot read
  templates or JSX source, it is slower, and it misses states that only
  appear after user interaction (open menus, modals, `:hover`).

A remover that deletes too much breaks people's sites. A remover that keeps
a little too much only leaves some bytes behind. The bug trackers of
existing tools show that false removals are what users fear most.

## Decision

prunecss uses a **conservative bag of names**:

- The scanner collects class names from content files into one set.
- A selector is kept unless it **requires** a class or id that is not in the
  set.
- These never cause a removal: tags, attributes, combinators, pseudo-classes
  and pseudo-elements, and anything inside `:not()`, `:has()` or
  `:nth-child(… of …)`.
- `:is()`, `:where()` and `:any()` are kept if any one alternative can match.
- Class names are compared exactly (case-sensitive) after CSS unescaping.
- At-rules that need usage tracking (`@keyframes`, `@font-face`, `@property`,
  custom properties) are kept for now. Named `@layer` blocks are always
  kept because they fix cascade order.

When we are not sure, we keep the rule.

## Consequences

- prunecss works on source files of any framework, not only rendered HTML.
- It is predictable: users can reason about why a rule was kept.
- Output is larger than DOM matching could produce. A stricter "precise HTML"
  mode may be added later as an option, never as the default.
- Class names built at runtime (`` `btn-${size}` ``) cannot be seen by any
  text scanner. Users will need a safelist for them.
- Every new rule type or selector feature must default to "keep" until we can
  prove it is unused.
