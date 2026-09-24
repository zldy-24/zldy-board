# Offline IME

Current stage: **Phase 1.5A.1 — ABI Versioning and Panic Contract Correction**.

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
- a versioned, opaque-handle C ABI with Rust-owned immutable results
- directional ABI minor-version negotiation and version-specific struct prefixes
- catchable-panic containment and structured FFI status codes
- C11 and C++17 Ubuntu smoke harnesses for linking, ownership, candidate flow,
  failed-commit recovery, and engine/session lifetime
- a small command-line REPL for exercising the Core

Not implemented yet:

- cross-platform ABI freeze or native linking validation beyond Ubuntu x86_64
- real Pinyin parsing and segmentation
- persistent dictionaries and `.imedict`
- persistent learning
- SQLite storage
- model inference
- Android, Windows, Linux, or iOS input-method adapters

The ABI contract, ownership rules, compatibility policy, build commands, and
native smoke-test instructions are in [`docs/ffi.md`](docs/ffi.md). The public
header is [`core/ime-ffi/include/ime.h`](core/ime-ffi/include/ime.h).

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
