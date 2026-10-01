# PR-only repository gates

The workflow implements checks. It **does not configure GitHub repository settings**, protect `main`, approve a PR, enable auto-merge, or merge anything. Verify the following settings on the actual repository before relying on them.

## Required repository configuration

1. Create an active branch ruleset targeting `main`.
2. Require a pull request before merging. Keep the bypass list empty (including administrators and automation), block force pushes and branch deletion, require conversation resolution, and require the `ci-gate` status check. Require branches to be up to date, or use a merge queue; the workflow supports `merge_group` checks.
3. Choose the human review requirement appropriate to the repository. A recorded author self-review is useful evidence but is not an independent approval, and GitHub does not let authors approve their own PR. If one external approval is required, auto-merge will wait for it.
4. Enable the repository's **Allow auto-merge** option. After self-review, use GitHub's built-in auto-merge on the PR. Do not create a write-token action that approves or merges its own code.
5. Consider requiring review by a trusted maintainer for `.github/workflows/`, `scripts/check_coverage.py`, and coverage configuration. Add verified maintainers to CODEOWNERS before enabling required code-owner reviews; do not invent an owner.
6. Check that the exact PR commit has passed `ci-gate`, that the ruleset is active, and that auto-merge is actually enabled. If repository administration access or plan support prevents this setup, report that blocker. A YAML file or checked checkbox is not evidence that protection is active.

## Check composition

`ci-gate` runs with `always()` and rejects failed, skipped, cancelled, or otherwise unsuccessful required jobs:

- Python syntax and coverage-checker regression tests
- Rust formatting, Clippy with warnings denied, native and shared-core WASM builds, generated-WASM Node smoke tests, unit/integration tests, LLVM line coverage
- Vue typechecking, production build, unit tests and LCOV coverage
- Sites Worker typechecking, build, unit tests and LCOV coverage
- Combined production line coverage **≥80%** and PR changed executable-line coverage **≥90%**

The Actions token is read-only, checkout credentials are not persisted, third-party Actions are pinned to full commits, and no untrusted PR runs through `pull_request_target`. No deployment or mutation is hidden in CI.

## Coverage contract

Run the checker from a clean checkout of the tested commit:

```sh
python3 scripts/check_coverage.py --base "$BASE_SHA" \
  --report coverage/rust.lcov . \
  --report frontend/coverage/lcov.info frontend \
  --report sites/coverage/lcov.info sites \
  --total-min 80 --changed-min 90 --json-output coverage/summary.json
```

Each `--report` supplies an LCOV file and the directory against which its relative `SF:` paths are resolved. Absolute paths must be inside this checkout. CI compares its checked-out PR merge commit with the event's PR base SHA; push and merge-queue runs use their event base. Initial history uses the empty Git tree. The first implementation PR therefore requires 90% coverage on all newly added executable lines.

Production sources are Rust files under `crates/*/src/` and JS/TS/Vue files under `frontend/src/` and `sites/src/`. Every current production file must have an LCOV `SF:` record, even if unchanged, so files omitted by a test runner fail the gate. A correctly reported zero-executable-line source is permitted. Changed executable lines are the instrumenter's physical `DA:` records; comments, types, and whitespace do not become guessed executable lines. Total coverage preserves provider `LF`/`LH` summaries. LLVM computes those summaries from functions/instantiations, which can count overlapping source lines differently from file-level `DA` records; equating the two would reject valid Rust reports or erase uncovered instances. When duplicate records are merged, physical hits are combined, but unmatched summary-only uncovered instances are conservatively retained. See the official [LLVM LCOV exporter](https://github.com/llvm/llvm-project/blob/main/llvm/tools/llvm-cov/CoverageExporterLcov.cpp) and [coverage summary implementation](https://github.com/llvm/llvm-project/blob/main/llvm/tools/llvm-cov/CoverageSummaryInfo.cpp). Missing source records, empty/truncated reports, summary-only nonempty files without `DA`, and impossible summary bounds fail rather than silently pass.

Only clearly named test files/directories, test fixtures, and TypeScript declaration files are outside the production denominator. Put Rust unit tests in dedicated `src/tests.rs` modules to avoid counting test code as application coverage. Build configuration and generated WASM bindings/assets are not handwritten runtime source. Handwritten worker adapters and entrypoints are covered; there is no runtime adapter exclusion.

The checker handles line deletion, renames, duplicate LCOV records, literal unusual filenames, and an optional `--working-tree` mode that also includes staged, unstaged, and untracked production source. Normal CI uses only committed code. It does not infer branch coverage from LCOV line coverage.

Run regression tests with:

```sh
python3 -m unittest discover -s scripts -p 'test_*.py' -v
```

## Pinned Actions

The following release-tag commit mappings were verified against each official GitHub repository's Git refs API on 2026-10-01:

- `actions/checkout` v4.2.2: `11bd71901bbe5b1630ceea73d27597364c9af683`
- `actions/upload-artifact` v4.6.2: `ea165f8d65b6e75b540449e92b4886f43607fa02`
- `actions/download-artifact` v4.3.0: `d3f86a106a0bac45b974a628896c90dbdf5c8093`
- `actions/setup-node` v4.4.0: `49933ea5288caeca8642d1e84afbd3f7d6820020`
- `oven-sh/setup-bun` v2.2.0: `0c5077e51419868618aeaa5fe8019c62421857d6`

Node uses the 24 release line (required for the Worker database tests). Bun is pinned to 1.4.2; cargo-llvm-cov to 0.9.1; wasm-bindgen-cli to 0.2.129. The Worker build consumes the tested frontend and WASM artifacts from its prerequisite jobs. Dependabot proposes Action/Cargo updates through PRs rather than modifying `main` directly.
