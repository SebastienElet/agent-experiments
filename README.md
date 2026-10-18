# agent-experiments

Bounded agent experiments, validation and comparison, extracted from Arnes.
Prepared for `@sebastienelet/agent-experiments@1.0.0`; no registry release is qualified yet.

Use Bun 1.4.3 to acquire the exact published version once available. Intended native
targets: macOS ARM64 and Linux x64 glibc. Git and standard Unix utilities are prerequisites.
Codex is needed only for explicit live runs and preflight; installation does not log in
or invoke a paid agent. `--help` exposes validation, fixture smoke, preflight, run and compare.

`--repository /absolute/corpus` selects the input corpus. It contains `cases.json`,
`fixtures/<id>.json`, and `instructions.md` (or an explicitly selected relative variant
under `variants/`). Case source paths and headings are relative to that corpus.
The current engine retains its two existing synthetic fixture identities and oracles;
it does not implement arbitrary evaluations. The small corpus under
`tests/fixtures/corpus` tests the engine; personal instructions and skill evaluations
remain with their owners.

`validate-evals` additionally reads `selection.json`, e.g.
`{"skills":["skills"],"historical_evidence":[]}`. Skill roots are explicit, tracked Git
paths. Empty discovery, missing sources, invalid triggers and numbered references outside
the exact historical allowlist are refused. The caller owns that allowlist; it is not
inferred from a dotfiles checkout. `validate-evidence` accepts report paths or reads the
corpus's `evidence/` directory. A report is validated against its own recorded evidence,
not current source bytes. Historical schema 1 reports remain readable without relabeling.

Live execution requires explicit model, case selection and report destination, bounds
runs/timeouts and refuses to overwrite a report. PASS, FAIL and INVALID remain distinct.
Comparisons reject incompatible reports. Raw transcripts are discarded. Synthetic smoke
is not evidence about live model quality. Installation and tests do not trigger live runs.

Contribute with Rust 1.98.1 and Bun 1.4.3: `bun install --frozen-lockfile`,
`cargo test --locked`, `bun run build`, `bun test tests/launcher.test.mjs`.
Native packages include executable SHA-256 and source revision in `BUILD.json`.
The manual publish workflow uses npm trusted publishers (OIDC) with provenance for all
three packages. Publish native variants before the entry package; inspect registry state
before resuming a partial publication. Preserve existing reports when installing or rolling back.
