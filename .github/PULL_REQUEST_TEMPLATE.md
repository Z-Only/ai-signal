## Summary

Describe what changed and why.

## Verification

- [ ] I reviewed the complete diff, including error paths and security implications
- [ ] Builds, static checks, and unit tests passed on the exact PR revision
- [ ] Production line coverage is at least 95%; added/changed executable lines at least 95%
- [ ] No runtime source is hidden from coverage; new source has LCOV records
- [ ] No secrets, generated dependencies, local database files, or credentials are committed
- [ ] Documentation, migrations, rollback considerations, and screenshots are included where applicable

## Self-review

Record findings and fixes, remaining risks, and any checks that were not run. Do not label unrun checks as passing.

## Merge

Enable GitHub's built-in auto-merge only after self-review. The protected `main` branch must require `ci-gate`; never bypass checks or push directly to `main`.
