#!/usr/bin/env bash
# Creates (or updates) the labels the issue forms, CONTRIBUTING.md and
# .github/labeler.yml use. Run once after creating the repository, with the
# GitHub CLI logged in:  scripts/labels.sh [owner/repo]
set -euo pipefail
repo="${1:-Anikhet/prunecss}"

label() { gh label create "$1" --repo "$repo" --color "$2" --description "$3" --force; }

# Kinds
label "bug"                "d73a4a" "Something isn't working"
label "false-removal"      "b60205" "CSS that a page uses was removed"
label "enhancement"        "a2eeef" "New feature or request"
label "question"           "d876e3" "Further information is requested"
label "dependencies"       "0366d6" "Dependency updates"
label "release"            "ededed" "Release pull requests from release-plz"

# Triage
label "needs-reproduction" "fbca04" "Waiting for a minimal example"
label "needs-decision"     "fbca04" "Maintainer decision needed before work starts"
label "good first issue"   "7057ff" "Good for newcomers"
label "help wanted"        "008672" "Extra attention is needed"

# Areas (applied automatically by .github/labeler.yml)
label "A-scanner"          "c5def5" "Finding class names in content"
label "A-pruner"           "c5def5" "Selector matching, safelist, tracking"
label "A-cli"              "c5def5" "The prunecss command and config"
label "A-docs"             "c5def5" "Documentation"
label "A-ci"               "c5def5" "CI, releases, supply chain"
label "A-bench"            "c5def5" "Benchmarks"
label "A-fuzz"             "c5def5" "Fuzzing"
