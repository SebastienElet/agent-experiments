# agent-experiments

Bounded agent experiments, validation and comparison, extracted from Arnes.
Prepared for `@sebastienelet/agent-experiments@0.0.1`; no registry release is qualified yet.

Use Bun 1.4.3 to acquire the exact published version once available. Intended native
targets: macOS ARM64 and Linux x64 glibc. Git and standard Unix utilities are prerequisites.
Codex is needed only for explicit live runs and preflight; installation does not log in
or invoke a paid agent. `--help` exposes validation, fixture smoke, preflight, run and compare.

`--repository /absolute/corpus` selects the input corpus. It contains `cases.json`,
`fixtures/<id>.json`, and `instructions.md` (or an explicitly selected relative variant
under `variants/`). Case source paths and headings are relative to that corpus.
The lookup engine retains its two existing synthetic fixture identities and oracles.
An explicit repository-trial mode additionally supports trusted synthetic mini-repositories. The small corpus under
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
Comparisons reject incompatible reports. The lookup modes discard raw transcripts. Synthetic smoke
is not evidence about live model quality. Installation and tests do not trigger live runs.

## Repository trials

`repository-run --case case.json --instructions instruction.txt --model <explicit-id>
--tool-path <explicit-PATH> --report <new-directory>` runs a fresh Codex workspace and
an independent executable oracle. `--timeout-seconds` bounds each setup, agent and oracle
process (1–600 seconds); `--network` explicitly permits sandbox networking when a dedicated
local test service requires it. These are trusted-code experiments, not an adversarial sandbox.
Never use private data or credentials in fixtures or retained stdout.

The case JSON has `id`, `prompt`, a nonempty `files` map (relative paths to UTF-8 contents),
`setup` (Bash), and an `oracle` map containing `run.sh`. Setup runs before the agent;
`run.sh` runs outside the agent's writable workspace, with the workspace as its working
directory and first argument. Oracle exit 0 means PASS, 10 means an observed assertion
failure (FAIL); any other exit, setup/provider failure, invalid protocol, timeout or output
limit is INVALID. Missing tools and syntax errors must not be relabeled exit 10.

Reports retain the frozen case, installed instruction, Codex JSONL stdout, oracle output,
pre-oracle source (`result.json`) and post-oracle source (`verified.json`), Unix modes and
SHA-256 fingerprints. Git metadata and `node_modules` are excluded; non-UTF-8 files,
symlinks, files over 4 MiB and snapshots over 512 files refuse retention. Stdout is capped
at 4 MiB per process; stderr is not retained unless a trusted script redirects it.
Reinstall dependencies and restore recorded modes when reconstructing a snapshot.
Reports are written only in a new directory; an interrupted directory is incomplete and
must be preserved separately, never overwritten as a successful run.

`repository-compare <baseline-directory> <candidate-directory>` verifies retained hashes
and rejects INVALID or incompatible cases, runner binaries, hosts, tools paths, model controls
and provider versions. It reports observed FAIL-to-PASS change only when instructions differ.
Fingerprints detect accidental changes, not hostile rewriting. The caller must record actual
tool versions and external service identities; an equal PATH does not prove those stayed fixed.
Use the same frozen prompt/oracle and fresh contexts, retain every attempt and distinguish
injected-instruction application from automatic skill activation. A pair is an observation,
not a statistical uplift estimate. Compare files are output, not permission to close an issue.

Contribute with Rust 1.98.1 and Bun 1.4.3: `bun install --frozen-lockfile`,
`cargo test --locked`, `bun run build`, `bun test tests/launcher.test.mjs`.
Native packages include executable SHA-256 and source revision in `BUILD.json`.
The manual publish workflow uses npm trusted publishers (OIDC) with provenance for all
three packages. Publish native variants before the entry package; inspect registry state
before resuming a partial publication. Preserve existing reports when installing or rolling back.
