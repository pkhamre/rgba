# Instructions for coding agents

Read [CONTRIBUTION.md](CONTRIBUTION.md) for the contributor workflow and
[docs/TESTING.md](docs/TESTING.md) for validation requirements and known gaps.
Apply the instructions below when working in this repository:

- Follow the Google Rust Style Guide and `docs/STYLE.md`.
- Keep verified behavior and accuracy gaps documented.
- Commit work in small, logical steps from now on. After each completed step, run relevant checks, review the staged diff, and create a descriptive commit. Do not accumulate unrelated steps in one commit.
- Keep each commit honest about validation: record known failures rather than claiming they pass. The optional full CPU reference comparison currently has documented mismatches; see `docs/TESTING.md`.
- Run `sh scripts/check.sh` before committing code changes, plus targeted checks for the changed behavior.
- Never commit BIOS/game images, downloaded test assets, generated traces/reports, toolchains, build products, or cartridge saves. Preserve required third-party license notices.
- Do not amend existing commits, rewrite history, or push unless requested by the user.
- For a requested merge request or GitHub pull request, summarize the changes, tests run, and any failures or unverified behavior. Do not submit one unless the user asks.
