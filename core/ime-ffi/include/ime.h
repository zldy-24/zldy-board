#ifndef ZLDY_IME_H
#define ZLDY_IME_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define IME_ABI_VERSION UINT32_C(0x00010000)

typedef uint32_t ImeStatus;
#define IME_STATUS_OK UINT32_C(0)
#define IME_STATUS_INVALID_ARGUMENT UINT32_C(1)
#define IME_STATUS_NULL_POINTER UINT32_C(2)
#define IME_STATUS_INVALID_UTF8 UINT32_C(3)
#define IME_STATUS_UNSUPPORTED UINT32_C(4)
#define IME_STATUS_STALE_REVISION UINT32_C(5)
#define IME_STATUS_BUDGET_EXCEEDED UINT32_C(6)
#define IME_STATUS_INTERNAL_PANIC UINT32_C(7)
#define IME_STATUS_INTERNAL_ERROR UINT32_C(8)
#define IME_STATUS_UNKNOWN_CANDIDATE UINT32_C(9)
#define IME_STATUS_UNKNOWN_ACTION UINT32_C(10)
#define IME_STATUS_UNSUPPORTED_ABI_VERSION UINT32_C(11)

typedef uint32_t ImeEventTag;
#define IME_EVENT_INSERT_TEXT UINT32_C(1)
#define IME_EVENT_BACKSPACE UINT32_C(2)
#define IME_EVENT_DELETE_FORWARD UINT32_C(3)
#define IME_EVENT_MOVE_COMPOSITION_CURSOR UINT32_C(4)
#define IME_EVENT_SELECT_CANDIDATE UINT32_C(5)
#define IME_EVENT_MOVE_CANDIDATE_SELECTION UINT32_C(6)
#define IME_EVENT_COMMIT UINT32_C(7)
#define IME_EVENT_CANCEL UINT32_C(8)
#define IME_EVENT_RESET UINT32_C(9)

typedef uint32_t ImeEventStatus;
#define IME_EVENT_STATUS_OK UINT32_C(0)
#define IME_EVENT_STATUS_NO_OP UINT32_C(1)
#define IME_EVENT_STATUS_UNSUPPORTED UINT32_C(2)

typedef uint32_t ImeEventHandling;
#define IME_EVENT_HANDLING_CONSUMED UINT32_C(0)
#define IME_EVENT_HANDLING_PASS_THROUGH UINT32_C(1)

typedef uint32_t ImeSessionPhase;
#define IME_PHASE_IDLE UINT32_C(0)
#define IME_PHASE_COMPOSING UINT32_C(1)
#define IME_PHASE_CANDIDATE_SELECTING UINT32_C(2)

typedef uint32_t ImeActionTag;
#define IME_ACTION_SET_COMPOSITION_TEXT UINT32_C(1)
#define IME_ACTION_FINISH_COMPOSITION UINT32_C(2)
#define IME_ACTION_CANCEL_COMPOSITION UINT32_C(3)
#define IME_ACTION_COMMIT_TEXT UINT32_C(4)
#define IME_ACTION_DELETE_BACKWARD UINT32_C(5)
#define IME_ACTION_REQUEST_PLATFORM_RESET UINT32_C(6)

typedef uint32_t ImeCommitKind;
#define IME_COMMIT_CANDIDATE UINT32_C(0)
#define IME_COMMIT_RAW_FALLBACK UINT32_C(1)
#define IME_COMMIT_DIRECT_INPUT UINT32_C(2)

typedef uint32_t ImeActionDisposition;
#define IME_ACTION_APPLIED UINT32_C(0)
#define IME_ACTION_REJECTED UINT32_C(1)
#define IME_ACTION_UNAVAILABLE UINT32_C(2)
#define IME_ACTION_SUPERSEDED UINT32_C(3)
#define IME_ACTION_FAILED UINT32_C(4)

typedef uint32_t ImePlatformResetReason;
#define IME_RESET_EXPLICIT UINT32_C(0)
#define IME_RESET_STATE_MISMATCH UINT32_C(1)

typedef uint32_t ImeDegradedComponent;
#define IME_DEGRADED_LANGUAGE UINT32_C(1)
#define IME_DEGRADED_DICTIONARY UINT32_C(2)
#define IME_DEGRADED_CANDIDATE UINT32_C(3)
#define IME_DEGRADED_RANKING UINT32_C(4)

typedef struct ImeEngineHandle ImeEngineHandle;
typedef struct ImeSessionHandle ImeSessionHandle;

typedef struct {
    const uint8_t *ptr;
    uint64_t len;
} ImeStringView;

/*
 * Common prefix of every versioned structure. A caller version is compatible
 * only when its major equals the library major and its minor is not newer than
 * the library minor. struct_size must cover the required prefix for the
 * declared version. Extra trailing bytes may be ignored, but do not imply
 * support for a newer ABI version.
 */
typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
} ImeStructHeader;

typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
    ImeEventTag tag;
    uint32_t reserved;
    ImeStringView text;
    int32_t delta;
    uint32_t candidate_id;
    uint64_t state_revision;
} ImeInputEvent;

typedef struct {
    uint32_t candidate_id;
    uint32_t reserved;
    ImeStringView text;
} ImeCandidate;

typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t action_id;
    uint64_t session_id;
    uint64_t originating_revision;
    ImeActionTag tag;
    ImeCommitKind commit_kind;
    ImeStringView text;
    uint64_t cursor_utf8_byte_offset;
    uint32_t operation_count;
    ImePlatformResetReason reset_reason;
} ImeAction;

typedef struct ImeResult {
    uint32_t struct_size;
    uint32_t abi_version;
    ImeEventStatus status;
    ImeEventHandling event_handling;
    uint64_t session_id;
    uint64_t state_revision;
    uint64_t resource_generation;
    ImeSessionPhase phase;
    uint32_t reserved;
    ImeStringView composition;
    uint64_t composition_cursor_grapheme;
    uint64_t composition_cursor_utf8_byte_offset;
    const ImeCandidate *candidates;
    uint64_t candidate_count;
    uint32_t has_selected_candidate;
    uint32_t selected_candidate_id;
    const ImeAction *actions;
    uint64_t action_count;
    uint32_t reconciliation_required;
    uint32_t reserved2;
    const ImeDegradedComponent *degraded_components;
    uint64_t degraded_component_count;
    const uint32_t *result_flags;
    uint64_t result_flag_count;
} ImeResult;

/* Pure, allocation-free getter for the compile-time ABI version constant. */
uint32_t ime_v1_get_abi_version(void);

/*
 * All functions below return ImeStatus, including destroy/free operations.
 * No Rust unwind crosses this boundary; a catchable panic is reported as
 * IME_STATUS_INTERNAL_PANIC. Fatal process failures and invalid caller
 * pointers are outside that guarantee.
 */
ImeStatus ime_v1_engine_create(ImeEngineHandle **out_engine);
ImeStatus ime_v1_engine_destroy(ImeEngineHandle *engine);

ImeStatus ime_v1_session_create(
    const ImeEngineHandle *engine,
    ImeSessionHandle **out_session
);
ImeStatus ime_v1_session_destroy(ImeSessionHandle *session);

ImeStatus ime_v1_session_process_event(
    ImeSessionHandle *session,
    const ImeInputEvent *event,
    ImeResult **out_result
);

ImeStatus ime_v1_session_snapshot(
    const ImeSessionHandle *session,
    ImeResult **out_result
);

ImeStatus ime_v1_session_ack_action(
    ImeSessionHandle *session,
    uint64_t action_id,
    ImeActionDisposition disposition
);

ImeStatus ime_v1_result_free(ImeResult *result);

#ifdef __cplusplus
}
#endif

#endif
