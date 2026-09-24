# C ABI contract

Phase 1.5A.1 exposes the platform-independent Core through the versioned header
`core/ime-ffi/include/ime.h`. The crate builds both `libime_ffi.so` and
`libime_ffi.a` on Linux. This is a low-level ABI for platform adapters; it is
not a Linux, Windows, Android, or iOS input method by itself.

## Versioning and layout

`IME_ABI_VERSION` is `0x00010000`, encoded as `major << 16 | minor`. Every
versioned structure begins with the common `ImeStructHeader` fields
`struct_size` and `abi_version` at offsets 0 and 4.

Version compatibility is directional:

- a caller major different from the library major is rejected with
  `IME_STATUS_UNSUPPORTED_ABI_VERSION`;
- a caller minor newer than the library minor is rejected with the same status;
- an older or equal caller minor may be accepted when the library knows the
  required prefix for that declared version.

Consequently, the production 1.0 library accepts caller 1.0 and rejects 1.1,
1.2, and 2.x. A future 1.1 library may support callers 1.0 and 1.1, but must
reject callers 1.2 and 2.x.

Structure size is checked independently from version compatibility:

- a caller must initialize `struct_size` to `sizeof(the_structure)`;
- the minimum required size is selected from the caller's declared ABI version;
- a structure smaller than that version's required prefix is rejected;
- a larger structure with a compatible declared version is accepted, and
  unknown trailing bytes are ignored;
- a larger `struct_size` never implies support for a newer ABI version;
- all structures use the target platform's normal C alignment. Do not pack
  them or change compiler structure-alignment options.

The ABI uses fixed-width integer fields. Counts and byte lengths use
`uint64_t`, not `size_t`, so the wire shape does not change between 32-bit and
64-bit targets. The library performs checked conversion to Rust's native
index type.

Linux x86_64 layout is covered by Rust tests. The expected sizes are 16 bytes
for `ImeStringView`, 48 for `ImeInputEvent`, 24 for `ImeCandidate`, 72 for
`ImeAction`, and 160 for `ImeResult`.

## Handles and lifetime

`ImeEngineHandle` and `ImeSessionHandle` are opaque. Create them with the
matching `*_create` function and destroy each live handle exactly once with
the matching `*_destroy` function. Destroying a null handle is a no-op.

A session retains the immutable Core resources it needs. It remains usable if
the engine handle that created it is destroyed first. Destroying the engine
does not invalidate existing sessions, but the engine handle cannot then be
used to create new sessions.

The caller must not forge, copy, inspect, double-destroy, or use a handle after
destruction. One session handle requires exclusive access: do not call it
concurrently or re-enter it from the same thread. Separate sessions own
separate mutable state and may be driven independently, subject to the host's
normal synchronization of its own data.

## Strings

`ImeStringView` is `{ const uint8_t *ptr; uint64_t len; }`:

- `len` is a byte count, not a character count;
- strings are UTF-8 and are not NUL-terminated;
- embedded NUL bytes are valid;
- `{NULL, 0}` is the canonical empty view;
- a null pointer with a positive length is rejected;
- non-empty input must point to readable storage for the duration of the call;
- current input text is bounded to 4096 bytes and invalid UTF-8 is rejected.

Input is copied before the call returns. Output views point into their owning
`ImeResult`; copy them if they must outlive that result.

## Results and ownership

On success, `ime_v1_session_process_event` and `ime_v1_session_snapshot`
return a complete immutable snapshot through `out_result`. The result owns the
composition bytes, candidate array and strings, action array and strings,
degraded-component array, and result-flag array. All of those pointers remain
valid until the single matching call to `ime_v1_result_free`.

Do not free nested pointers, mutate them, or access them after freeing the
result. `ime_v1_result_free(NULL)` is a no-op. If a function returns a status
other than `IME_STATUS_OK`, an otherwise valid output slot is initialized to
null and must not be freed.

Empty arrays and strings use null pointers with zero counts. Current result
serialization is bounded to 64 candidates, 64 actions, and 64 KiB of combined
composition/candidate/action text. Exceeding a bound returns
`IME_STATUS_BUDGET_EXCEEDED` rather than producing a partial result.

## Event and action flow

Initialize every `ImeInputEvent` with zeroed storage, then set
`struct_size`, `abi_version`, and `tag`. Only fields belonging to the selected
tag are read. The C ABI currently supports:

- insert text;
- backward and forward deletion;
- composition-cursor movement;
- candidate selection and candidate-selection movement;
- commit, cancel, and reset.

Candidate selection must send both the candidate ID and the state revision
from the result that supplied it. A stale revision returns
`IME_STATUS_STALE_REVISION`; an unknown ID in the current revision returns
`IME_STATUS_UNKNOWN_CANDIDATE`.

The host must perform each returned `ImeAction` and acknowledge it with
`ime_v1_session_ack_action`. A failed commit acknowledgement can restore the
bounded recovery snapshot, including composition and candidates. The next
snapshot is the authoritative state. Action IDs are scoped to their session;
unknown or already-resolved IDs return `IME_STATUS_UNKNOWN_ACTION`.

## Errors and panic boundary

No Rust unwind may cross the C ABI boundary.

All APIs other than `ime_v1_get_abi_version` return `ImeStatus`. For these
status-returning APIs, including engine/session destruction and result free, a
catchable Rust panic is translated to `IME_STATUS_INTERNAL_PANIC`. Cleanup
paths are designed not to panic; null cleanup handles are successful no-ops.

`ime_v1_get_abi_version` does not have an `ImeStatus` return channel. It is a
pure, allocation-free function that returns the compile-time ABI version
constant, so the contract does not claim that it reports
`IME_STATUS_INTERNAL_PANIC`.

This is containment, not a guarantee of recovery from `panic=abort`, fatal
allocator failure, SIGSEGV/access violation, invalid caller pointers,
use-after-free, data races, stack failure, process termination, or other
undefined behavior.

As with any C API, the library cannot validate arbitrary pointer provenance.
Non-null arguments and output slots must be live, correctly aligned, and
readable or writable for the documented extent. Violating that requirement,
using freed pointers, or racing one session is caller-side undefined behavior.

## Build and smoke tests

From the repository root:

```bash
cargo build -p ime-ffi
mkdir -p target/ffi-smoke
cc -std=c11 -Wall -Wextra -Werror \
  -Icore/ime-ffi/include tests/ffi-smoke/c/smoke.c \
  -Ltarget/debug -lime_ffi -Wl,-rpath,'$ORIGIN/../debug' \
  -o target/ffi-smoke/c_smoke
c++ -std=c++17 -Wall -Wextra -Werror \
  -Icore/ime-ffi/include tests/ffi-smoke/cpp/smoke.cpp \
  -Ltarget/debug -lime_ffi -Wl,-rpath,'$ORIGIN/../debug' \
  -o target/ffi-smoke/cpp_smoke
target/ffi-smoke/c_smoke
target/ffi-smoke/cpp_smoke
```

The C harness covers the full Phase 1.5A path: create engine/session, destroy
the engine first, insert `nihao`, inspect candidates, select `你好`, acknowledge
the commit as failed, and verify recovery. The C++ harness verifies header
compatibility, linking, result ownership, and UTF-8 copying.

## Deliberately deferred

This ABI does not implement hardware-key normalization, a production Pinyin
parser, persistent dictionaries, `.imedict`, SQLite, learning, model
inference, networking, or any operating-system input-method adapter. Engine
creation currently loads the deterministic in-memory Phase 1B reference
dictionary used by tests and examples.
