# Security policy

## Supported versions

prunecss is still at 0.x. Only the latest 0.x minor release gets security
fixes. For example, once 0.3.0 is out, fixes go into 0.3.x and 0.2.x gets no
more updates.

| Version              | Supported |
| -------------------- | --------- |
| latest 0.x minor     | yes       |
| older 0.x releases   | no        |

This covers both published crates: `prunecss` (the command-line tool) and
`prunecss-core` (the library).

## Reporting a vulnerability

Please **do not** open a public issue, discussion or pull request for a
security problem.

Report it privately through GitHub instead:

1. Go to the [Security tab](https://github.com/Anikhet/prunecss/security) of
   the repository.
2. Click **Report a vulnerability**.
3. Fill in the form.

Only the maintainer can see the report. GitHub turns it into a draft
advisory where we can talk, share a fix and credit you.

Please include:

- the prunecss version (`prunecss --version`) and your operating system;
- the exact command, and any config file you used;
- the smallest input files (CSS, HTML, JS) that show the problem;
- what happens (crash, hang, file written to the wrong place, and so on).

If you used an AI tool or scanner to find the problem, say so, and check the
report yourself before you send it. Please only send issues you have
reproduced.

## What to expect

prunecss is maintained by one person in their spare time. The targets are:

- **within 14 days**: a first reply that confirms we received the report;
- **as soon as practical after that**: an assessment, and a fix or a plan.

We follow coordinated disclosure. Please give us a reasonable time to release
a fix before you share details publicly. We will agree on a disclosure date
with you, and credit you in the advisory unless you prefer not to be named.

For a confirmed vulnerability in a published crate, we will:

1. release a fixed version on crates.io;
2. publish a GitHub security advisory;
3. ask for a [RustSec](https://rustsec.org/) advisory, so that `cargo audit`
   and `cargo deny` warn people who use an affected version.

## What counts as a vulnerability

prunecss reads CSS, HTML and other content files and writes CSS. Those inputs
may come from places you do not fully trust, such as third-party themes or
pull requests. So these count as security issues:

- **crashes or panics** on crafted input;
- **hangs, or very slow runs**, on crafted input (for example, run time that
  grows much faster than the size of the input), or memory use that grows
  without bound;
- **writing files outside the place you asked for**, for example through path
  traversal (`../`) or symbolic links when writing output;
- **a malicious config file** that makes prunecss read or write files it
  should not, or run commands (once config files exist);
- a vulnerability in one of our dependencies that affects prunecss.

## What is not a vulnerability

- **CSS that prunecss removed but your page needed.** This is a normal
  correctness bug, not a security issue. Please open a public
  ["CSS was wrongly removed" issue](https://github.com/Anikhet/prunecss/issues/new?template=false_removal.yml)
  so others can see it and help.
- CSS that was kept but is unused.
- Problems that need someone to already control your machine, your shell or
  the prunecss binary itself.
