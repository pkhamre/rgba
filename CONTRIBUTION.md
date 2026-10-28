# Contributing

Contributions, including bug fixes, tests, documentation, and accuracy research,
are welcome. This repository uses GitHub; a GitHub pull request is the equivalent
of a merge request. For substantial changes, consider opening an issue first to
explain the problem and proposed approach.

## Prepare a change

1. Fork the repository if you do not have permission to create a branch, then
   clone your fork. Start a branch from the target branch (normally `main`):

   ```sh
   git switch main
   git pull --ff-only
   git switch -c my-change
   ```

2. Keep the change focused and make small, descriptive commits. Follow the
   Google Rust Style Guide and [docs/STYLE.md](docs/STYLE.md). Add or update
   tests for changed behavior, and document verified behavior and accuracy gaps.
3. Run the checks below and review your diff before committing. Do not commit
   BIOS or game images, downloaded test assets, cartridge saves, generated
   traces or reports, toolchains, or build products. Preserve third-party license
   notices and identify the source and license of any material you add.

## Validate

Install the prerequisites in the README's [build instructions](README.md#build-from-source).
For code changes, run the standard checks and targeted tests for the behavior
changed:

```sh
sh scripts/check.sh
sh scripts/cargo.sh test --locked --workspace --release
```

The release workspace tests are an additional recommended check. The standard
checks use synthetic data and do not require BIOS or game images. See
[docs/TESTING.md](docs/TESTING.md) for targeted and optional tests, prerequisites,
and known CPU-reference mismatches. Report what you actually ran, including
failures, skipped checks, and behavior you could not verify; do not imply that
passing tests establishes hardware accuracy. Review logs and other material for
private paths or copyrighted data before sharing them.

## Open a merge request

1. Push your branch to your fork or the repository, as appropriate, and open a
   GitHub pull request against the intended target branch. Link any relevant issue.
2. Describe the problem, the changes, and any user-visible effects. List the
   checks you ran and their results, along with known failures, limitations, and
   any manual testing still needed. Include reproduction steps for bug fixes
   where useful.
3. Respond to review comments and make follow-up changes on the same branch.
   Keep the request focused and update the description if its scope or validation
   changes. A maintainer reviews and merges the request; do not assume a
   particular approval or merge policy beyond the repository's current settings.

Coding agents should also follow [AGENTS.md](AGENTS.md). Its restrictions on
pushing, rewriting history, and submitting requests apply to agent work.
