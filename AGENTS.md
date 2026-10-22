# Project instructions

- Follow the Google Rust Style Guide and `docs/STYLE.md`.
- Follow the agreed milestones in `PLAN.md`; keep verified behavior and accuracy gaps documented.
- Commit work in small, logical steps from now on. After each completed step, run relevant checks, review the staged diff, and create a descriptive commit. Do not accumulate unrelated steps in one commit.
- Keep each commit honest about validation: record known failures rather than claiming they pass. The optional full CPU reference comparison currently has documented mismatches; see `docs/TESTING.md`.
- Run `sh scripts/check.sh` before committing code changes, plus targeted checks for the changed behavior.
- Never commit BIOS/game images, downloaded test assets, generated traces/reports, toolchains, build products, or cartridge saves. Preserve required third-party license notices.
- Do not amend existing commits, rewrite history, or push unless requested by the user.
