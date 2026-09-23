# Offline IME

Current stage: **Phase 1A.1 — Commit Action Recovery**.

Implemented:

- shared `ImeEngine` / `ImeSession` lifecycle
- Unicode-grapheme-aware `Composer`
- `Idle` / `Composing` state machine
- logical `InputEvent` types and minimal hardware-key normalization
- foundational `ImeState`, `ImeResult`, `ImeAction`, and action acknowledgement
- bounded, revision-guarded recovery for immediately failed `CommitText` actions
- a small command-line REPL for exercising the Core

Not implemented yet:

- dictionaries
- candidates and ranking
- persistent learning
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

Manual acknowledgement dispositions are `applied`, `failed`, `rejected`, `unavailable`, and
`superseded`.
