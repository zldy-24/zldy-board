#include "ime.h"

#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>

namespace {

[[noreturn]] void fail(const char *message) {
    std::cerr << "cpp_smoke: FAIL: " << message << '\n';
    std::exit(EXIT_FAILURE);
}

void check(bool condition, const char *message) {
    if (!condition) {
        fail(message);
    }
}

ImeInputEvent event_with_tag(ImeEventTag tag) {
    ImeInputEvent event{};
    event.struct_size = sizeof(event);
    event.abi_version = IME_ABI_VERSION;
    event.tag = tag;
    return event;
}

std::string copy_view(ImeStringView view) {
    if (view.len == 0) {
        return {};
    }
    check(view.ptr != nullptr, "non-empty view has a null pointer");
    return {reinterpret_cast<const char *>(view.ptr),
            static_cast<std::size_t>(view.len)};
}

}  // namespace

int main() {
    ImeEngineHandle *engine = nullptr;
    ImeSessionHandle *session = nullptr;
    ImeResult *result = nullptr;

    check(ime_v1_get_abi_version() == IME_ABI_VERSION, "ABI version mismatch");
    check(ime_v1_engine_create(&engine) == IME_STATUS_OK, "engine creation");
    check(ime_v1_session_create(engine, &session) == IME_STATUS_OK,
          "session creation");

    const std::string input{"nihao"};
    ImeInputEvent insert = event_with_tag(IME_EVENT_INSERT_TEXT);
    insert.text.ptr = reinterpret_cast<const uint8_t *>(input.data());
    insert.text.len = input.size();
    check(ime_v1_session_process_event(session, &insert, &result) == IME_STATUS_OK,
          "insert event");
    check(result != nullptr, "insert result");
    check(copy_view(result->composition) == input, "composition round trip");
    check(result->candidate_count > 0, "candidate count");
    const std::string candidate = copy_view(result->candidates[0].text);
    check(candidate == "\xE4\xBD\xA0\xE5\xA5\xBD", "candidate text");
    check(ime_v1_result_free(result) == IME_STATUS_OK, "result ownership/free");

    check(ime_v1_session_destroy(session) == IME_STATUS_OK, "session destroy");
    check(ime_v1_engine_destroy(engine) == IME_STATUS_OK, "engine destroy");

    std::cout << "cpp_smoke: header/link/ownership=PASS\n";
    std::cout << "cpp_smoke: candidate=" << candidate << '\n';
    std::cout << "cpp_smoke: PASS\n";
    return EXIT_SUCCESS;
}
