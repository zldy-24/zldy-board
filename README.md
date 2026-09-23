# Offline IME

Current stage: **Phase 1B — Reference Language and Candidate Pipeline**.

Implemented:

- shared `ImeEngine` / `ImeSession` lifecycle
- Unicode-grapheme-aware `Composer`
- `Idle` / `Composing` state machine
- logical `InputEvent` types and minimal hardware-key normalization
- foundational `ImeState`, `ImeResult`, `ImeAction`, and action acknowledgement
- bounded, revision-guarded recovery for immediately failed `CommitText` actions
- ASCII-case-normalizing `ReferenceLanguageEngine`
- deterministic, read-only `InMemoryDictionary`
- bounded dictionary-backed candidate provider and candidate pipeline
- Unicode-text deduplication and deterministic integer ranking
- revision-scoped candidate snapshots, selection, and candidate commit recovery
- a small command-line REPL for exercising the Core

Not implemented yet:

- real Pinyin parsing and segmentation
- persistent dictionaries and `.imedict`
- persistent learning
- SQLite storage
- model inference
- C ABI / FFI
- Android, Windows, Linux, or iOS input-method adapters

Run the checks:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run the CLI:

```bash
cargo run -p ime-cli
```

The CLI acknowledges actions as `Applied` by default. To exercise failure paths manually:

```text
autoack off
text nihao
commit
ack 1 failed
state
```

Candidate flow can be exercised with:

```text
text NIHAO
candidates
select 1
```

Manual acknowledgement dispositions are `applied`, `failed`, `rejected`, `unavailable`, and
`superseded`.
