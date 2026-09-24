#include "ime.h"

#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__linux__) && defined(__x86_64__)
_Static_assert(sizeof(ImeStructHeader) == 8, "ImeStructHeader size");
_Static_assert(_Alignof(ImeStructHeader) == 4, "ImeStructHeader alignment");
_Static_assert(sizeof(ImeStringView) == 16, "ImeStringView size");
_Static_assert(_Alignof(ImeStringView) == 8, "ImeStringView alignment");
_Static_assert(sizeof(ImeInputEvent) == 48, "ImeInputEvent size");
_Static_assert(_Alignof(ImeInputEvent) == 8, "ImeInputEvent alignment");
_Static_assert(offsetof(ImeInputEvent, text) == 16, "ImeInputEvent.text offset");
_Static_assert(offsetof(ImeInputEvent, state_revision) == 40,
               "ImeInputEvent.state_revision offset");
_Static_assert(sizeof(ImeCandidate) == 24, "ImeCandidate size");
_Static_assert(sizeof(ImeAction) == 72, "ImeAction size");
_Static_assert(offsetof(ImeAction, text) == 40, "ImeAction.text offset");
_Static_assert(sizeof(ImeResult) == 160, "ImeResult size");
_Static_assert(offsetof(ImeResult, composition) == 48,
               "ImeResult.composition offset");
_Static_assert(offsetof(ImeResult, actions) == 104, "ImeResult.actions offset");
#endif

typedef struct {
    ImeInputEvent event;
    uint64_t trailing_sentinel[2];
} ExtendedInputEvent;

#define CHECK(condition, message)                                              \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "c_smoke: FAIL: %s (line %d)\n", message, __LINE__); \
            exit(EXIT_FAILURE);                                                \
        }                                                                      \
    } while (0)

static ImeInputEvent event_with_tag(ImeEventTag tag) {
    ImeInputEvent event;
    memset(&event, 0, sizeof(event));
    event.struct_size = (uint32_t)sizeof(event);
    event.abi_version = IME_ABI_VERSION;
    event.tag = tag;
    return event;
}

static int view_equals(ImeStringView view, const char *expected) {
    const size_t expected_len = strlen(expected);
    return view.len == (uint64_t)expected_len &&
           (expected_len == 0 || memcmp(view.ptr, expected, expected_len) == 0);
}

int main(void) {
    ImeEngineHandle *engine = NULL;
    ImeSessionHandle *session = NULL;
    ImeResult *result = NULL;
    const char input[] = "nihao";
    const char expected_candidate[] = "\xE4\xBD\xA0\xE5\xA5\xBD";

    CHECK(ime_v1_get_abi_version() == IME_ABI_VERSION, "ABI version mismatch");
    CHECK(ime_v1_engine_create(&engine) == IME_STATUS_OK, "engine creation");
    CHECK(engine != NULL, "engine handle");
    CHECK(ime_v1_session_create(engine, &session) == IME_STATUS_OK,
          "session creation");
    CHECK(session != NULL, "session handle");

    /* A session owns the immutable resources it needs after construction. */
    CHECK(ime_v1_engine_destroy(engine) == IME_STATUS_OK,
          "destroy engine before session");
    engine = NULL;

    ImeInputEvent current_version = event_with_tag(IME_EVENT_BACKSPACE);
    CHECK(ime_v1_session_process_event(session, &current_version, &result) ==
              IME_STATUS_OK,
          "current ABI version accepted");
    CHECK(ime_v1_result_free(result) == IME_STATUS_OK,
          "free current-version result");
    result = NULL;

    ImeInputEvent future_minor = current_version;
    future_minor.abi_version = UINT32_C(0x00010001);
    CHECK(ime_v1_session_process_event(session, &future_minor, &result) ==
              IME_STATUS_UNSUPPORTED_ABI_VERSION,
          "future ABI minor rejected");
    CHECK(result == NULL, "future-minor result remains null");

    ImeInputEvent future_major = current_version;
    future_major.abi_version = UINT32_C(0x00020000);
    CHECK(ime_v1_session_process_event(session, &future_major, &result) ==
              IME_STATUS_UNSUPPORTED_ABI_VERSION,
          "future ABI major rejected");
    CHECK(result == NULL, "future-major result remains null");

    ExtendedInputEvent extended;
    memset(&extended, 0, sizeof(extended));
    extended.event = current_version;
    extended.event.struct_size = (uint32_t)sizeof(extended);
    extended.trailing_sentinel[0] = UINT64_C(0x1122334455667788);
    extended.trailing_sentinel[1] = UINT64_C(0x8877665544332211);
    CHECK(ime_v1_session_process_event(session, &extended.event, &result) ==
              IME_STATUS_OK,
          "same-version trailing bytes accepted");
    CHECK(extended.trailing_sentinel[0] == UINT64_C(0x1122334455667788) &&
              extended.trailing_sentinel[1] == UINT64_C(0x8877665544332211),
          "trailing bytes ignored");
    CHECK(ime_v1_result_free(result) == IME_STATUS_OK,
          "free extended-structure result");
    result = NULL;

    ImeInputEvent insert = event_with_tag(IME_EVENT_INSERT_TEXT);
    insert.text.ptr = (const uint8_t *)input;
    insert.text.len = (uint64_t)(sizeof(input) - 1U);
    CHECK(ime_v1_session_process_event(session, &insert, &result) == IME_STATUS_OK,
          "insert nihao");
    CHECK(result != NULL, "insert result");
    CHECK(result->phase == IME_PHASE_CANDIDATE_SELECTING,
          "candidate-selecting phase");
    CHECK(view_equals(result->composition, input), "composition text");
    CHECK(result->candidate_count >= UINT64_C(1), "candidate count");
    CHECK(result->candidates[0].candidate_id == UINT32_C(1),
          "fixed vector candidate id");
    CHECK(view_equals(result->candidates[0].text, expected_candidate),
          "first candidate is nihao");

    const uint32_t candidate_id = result->candidates[0].candidate_id;
    const uint64_t candidate_revision = result->state_revision;
    CHECK(ime_v1_result_free(result) == IME_STATUS_OK, "free insert result");
    result = NULL;

    ImeInputEvent select = event_with_tag(IME_EVENT_SELECT_CANDIDATE);
    select.candidate_id = candidate_id;
    select.state_revision = candidate_revision;
    CHECK(ime_v1_session_process_event(session, &select, &result) == IME_STATUS_OK,
          "select candidate");
    CHECK(result != NULL, "selection result");
    CHECK(result->action_count >= UINT64_C(1), "commit action count");
    CHECK(result->actions[0].tag == IME_ACTION_COMMIT_TEXT, "commit action tag");
    CHECK(result->actions[0].commit_kind == IME_COMMIT_CANDIDATE,
          "candidate commit kind");
    CHECK(view_equals(result->actions[0].text, expected_candidate),
          "commit action text");
    const uint64_t action_id = result->actions[0].action_id;
    CHECK(ime_v1_result_free(result) == IME_STATUS_OK, "free selection result");
    result = NULL;

    CHECK(ime_v1_session_ack_action(session, action_id, IME_ACTION_FAILED) ==
              IME_STATUS_OK,
          "ack failed commit");
    CHECK(ime_v1_session_snapshot(session, &result) == IME_STATUS_OK,
          "snapshot after failed commit");
    CHECK(result != NULL, "recovery snapshot");
    CHECK(result->phase == IME_PHASE_CANDIDATE_SELECTING,
          "failed commit restores candidate selection");
    CHECK(view_equals(result->composition, input),
          "failed commit restores composition");
    CHECK(result->candidate_count >= UINT64_C(1),
          "failed commit restores candidates");
    CHECK(view_equals(result->candidates[0].text, expected_candidate),
          "failed commit regenerates first candidate");
    CHECK(ime_v1_result_free(result) == IME_STATUS_OK, "free recovery result");
    result = NULL;

    CHECK(ime_v1_session_destroy(session) == IME_STATUS_OK, "session destroy");

    printf("c_smoke: ABI 0x%08x\n", ime_v1_get_abi_version());
    printf("c_smoke: version contract=PASS\n");
    printf("c_smoke: candidate=\xE4\xBD\xA0\xE5\xA5\xBD\n");
    printf("c_smoke: failed commit recovery=PASS\n");
    printf("c_smoke: engine-before-session lifetime=PASS\n");
    printf("c_smoke: PASS\n");
    return EXIT_SUCCESS;
}
