//! Versioned C ABI boundary for the platform-independent input-method Core.

#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};

use ime_core::{
    ActionDisposition, ActionId, CandidateId, CommitKind, DegradedComponent, EngineConfig,
    EventHandling, ImeAction as CoreAction, ImeActionKind, ImeEngine, ImeError,
    ImeResult as CoreResult, ImeSession, ImeState, ImeStatus as CoreEventStatus, InputEvent,
    LexemeId, PlatformResetReason, SessionPhase, StateRevision,
    dictionary::{DictionaryEntry, InMemoryDictionary},
};

pub const IME_ABI_VERSION: u32 = 0x0001_0000;

pub const IME_STATUS_OK: u32 = 0;
pub const IME_STATUS_INVALID_ARGUMENT: u32 = 1;
pub const IME_STATUS_NULL_POINTER: u32 = 2;
pub const IME_STATUS_INVALID_UTF8: u32 = 3;
pub const IME_STATUS_UNSUPPORTED: u32 = 4;
pub const IME_STATUS_STALE_REVISION: u32 = 5;
pub const IME_STATUS_BUDGET_EXCEEDED: u32 = 6;
pub const IME_STATUS_INTERNAL_PANIC: u32 = 7;
pub const IME_STATUS_INTERNAL_ERROR: u32 = 8;
pub const IME_STATUS_UNKNOWN_CANDIDATE: u32 = 9;
pub const IME_STATUS_UNKNOWN_ACTION: u32 = 10;
pub const IME_STATUS_UNSUPPORTED_ABI_VERSION: u32 = 11;

const IME_EVENT_INSERT_TEXT: u32 = 1;
const IME_EVENT_BACKSPACE: u32 = 2;
const IME_EVENT_DELETE_FORWARD: u32 = 3;
const IME_EVENT_MOVE_COMPOSITION_CURSOR: u32 = 4;
const IME_EVENT_SELECT_CANDIDATE: u32 = 5;
const IME_EVENT_MOVE_CANDIDATE_SELECTION: u32 = 6;
const IME_EVENT_COMMIT: u32 = 7;
const IME_EVENT_CANCEL: u32 = 8;
const IME_EVENT_RESET: u32 = 9;

const IME_EVENT_STATUS_OK: u32 = 0;
const IME_EVENT_STATUS_NO_OP: u32 = 1;
const IME_EVENT_STATUS_UNSUPPORTED: u32 = 2;

const IME_EVENT_HANDLING_CONSUMED: u32 = 0;
const IME_EVENT_HANDLING_PASS_THROUGH: u32 = 1;

const IME_PHASE_IDLE: u32 = 0;
const IME_PHASE_COMPOSING: u32 = 1;
const IME_PHASE_CANDIDATE_SELECTING: u32 = 2;

const IME_ACTION_SET_COMPOSITION_TEXT: u32 = 1;
const IME_ACTION_FINISH_COMPOSITION: u32 = 2;
const IME_ACTION_CANCEL_COMPOSITION: u32 = 3;
const IME_ACTION_COMMIT_TEXT: u32 = 4;
const IME_ACTION_DELETE_BACKWARD: u32 = 5;
const IME_ACTION_REQUEST_PLATFORM_RESET: u32 = 6;

const IME_COMMIT_CANDIDATE: u32 = 0;
const IME_COMMIT_RAW_FALLBACK: u32 = 1;
const IME_COMMIT_DIRECT_INPUT: u32 = 2;

const IME_ACTION_APPLIED: u32 = 0;
const IME_ACTION_REJECTED: u32 = 1;
const IME_ACTION_UNAVAILABLE: u32 = 2;
const IME_ACTION_SUPERSEDED: u32 = 3;
const IME_ACTION_FAILED: u32 = 4;

const IME_RESET_EXPLICIT: u32 = 0;
const IME_RESET_STATE_MISMATCH: u32 = 1;

const IME_DEGRADED_LANGUAGE: u32 = 1;
const IME_DEGRADED_DICTIONARY: u32 = 2;
const IME_DEGRADED_CANDIDATE: u32 = 3;
const IME_DEGRADED_RANKING: u32 = 4;

const MAX_FFI_INPUT_BYTES: u64 = 4 * 1024;
const MAX_RESULT_TEXT_BYTES: usize = 64 * 1024;
const MAX_RESULT_CANDIDATES: usize = 64;
const MAX_RESULT_ACTIONS: usize = 64;

/// Opaque C handle owning one Rust Engine.
#[repr(C)]
pub struct ImeEngineHandle {
    inner: ImeEngine,
}

/// Opaque C handle owning one Rust Session.
#[repr(C)]
pub struct ImeSessionHandle {
    inner: ImeSession,
}

/// Non-NUL-terminated UTF-8 bytes with an explicit fixed-width length.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ImeStringView {
    pub ptr: *const u8,
    pub len: u64,
}

/// Common prefix for every versioned C structure.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ImeStructHeader {
    pub struct_size: u32,
    pub abi_version: u32,
}

/// Tagged input event. Unknown trailing fields are ignored for a compatible declared version.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ImeInputEvent {
    pub struct_size: u32,
    pub abi_version: u32,
    pub tag: u32,
    pub reserved: u32,
    pub text: ImeStringView,
    pub delta: i32,
    pub candidate_id: u32,
    pub state_revision: u64,
}

/// Minimal candidate view exposed across the C ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ImeCandidate {
    pub candidate_id: u32,
    pub reserved: u32,
    pub text: ImeStringView,
}

/// One platform side effect produced by the Core.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ImeAction {
    pub struct_size: u32,
    pub abi_version: u32,
    pub action_id: u64,
    pub session_id: u64,
    pub originating_revision: u64,
    pub tag: u32,
    pub commit_kind: u32,
    pub text: ImeStringView,
    pub cursor_utf8_byte_offset: u64,
    pub operation_count: u32,
    pub reset_reason: u32,
}

/// Complete immutable result returned by one hot-path operation or snapshot request.
#[repr(C)]
#[derive(Debug)]
pub struct ImeResult {
    pub struct_size: u32,
    pub abi_version: u32,
    pub status: u32,
    pub event_handling: u32,
    pub session_id: u64,
    pub state_revision: u64,
    pub resource_generation: u64,
    pub phase: u32,
    pub reserved: u32,
    pub composition: ImeStringView,
    pub composition_cursor_grapheme: u64,
    pub composition_cursor_utf8_byte_offset: u64,
    pub candidates: *const ImeCandidate,
    pub candidate_count: u64,
    pub has_selected_candidate: u32,
    pub selected_candidate_id: u32,
    pub actions: *const ImeAction,
    pub action_count: u64,
    pub reconciliation_required: u32,
    pub reserved2: u32,
    pub degraded_components: *const u32,
    pub degraded_component_count: u64,
    pub result_flags: *const u32,
    pub result_flag_count: u64,
}

#[repr(C)]
struct OwnedResult {
    wire: ImeResult,
    _composition: Vec<u8>,
    _candidate_texts: Vec<Vec<u8>>,
    _candidates: Box<[ImeCandidate]>,
    _action_texts: Vec<Vec<u8>>,
    _actions: Box<[ImeAction]>,
    _degraded_components: Box<[u32]>,
    _result_flags: Box<[u32]>,
}

struct ResultMetadata {
    status: u32,
    event_handling: u32,
    session_id: u64,
    state_revision: u64,
    resource_generation: u64,
    reconciliation_required: bool,
}

struct WireActionKind {
    tag: u32,
    commit_kind: u32,
    text: Vec<u8>,
    cursor_utf8_byte_offset: u64,
    operation_count: u32,
    reset_reason: u32,
}

/// Returns ABI version `major << 16 | minor`.
///
/// This function performs no allocation and returns a compile-time constant.
#[unsafe(no_mangle)]
pub extern "C" fn ime_v1_get_abi_version() -> u32 {
    IME_ABI_VERSION
}

/// Creates an Engine using the Phase 1.5A fixed reference dictionary.
///
/// # Safety
///
/// `out_engine` must be a live, aligned, writable pointer slot for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_engine_create(out_engine: *mut *mut ImeEngineHandle) -> u32 {
    ffi_boundary(|| {
        prepare_out(out_engine)?;
        let engine =
            ImeEngine::with_reference_dictionary(EngineConfig::default(), reference_dictionary())
                .map_err(map_core_error)?;
        store_out(
            out_engine,
            Box::into_raw(Box::new(ImeEngineHandle { inner: engine })),
        )
    })
}

/// Destroys an Engine handle. A null handle is accepted as a no-op.
///
/// # Safety
///
/// A non-null `engine` must be a live handle returned by this crate and not previously destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_engine_destroy(engine: *mut ImeEngineHandle) -> u32 {
    ffi_boundary(|| {
        if !engine.is_null() {
            // SAFETY: the ABI contract requires a live pointer returned by engine_create exactly once.
            unsafe { drop(Box::from_raw(engine)) };
        }
        Ok(())
    })
}

/// Creates an isolated Session that internally retains the Engine resources.
///
/// # Safety
///
/// `engine` must be a live handle and `out_session` must be a live, aligned, writable pointer slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_session_create(
    engine: *const ImeEngineHandle,
    out_session: *mut *mut ImeSessionHandle,
) -> u32 {
    ffi_boundary(|| {
        prepare_out(out_session)?;
        let engine = require_ref(engine)?;
        let session = engine.inner.new_session().map_err(map_core_error)?;
        store_out(
            out_session,
            Box::into_raw(Box::new(ImeSessionHandle { inner: session })),
        )
    })
}

/// Destroys a Session handle. A null handle is accepted as a no-op.
///
/// # Safety
///
/// A non-null `session` must be a live handle returned by this crate and not previously destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_session_destroy(session: *mut ImeSessionHandle) -> u32 {
    ffi_boundary(|| {
        if !session.is_null() {
            // SAFETY: the ABI contract requires a live pointer returned by session_create exactly once.
            unsafe { drop(Box::from_raw(session)) };
        }
        Ok(())
    })
}

/// Processes one event and returns a Rust-owned full result snapshot.
///
/// # Safety
///
/// All non-null pointers must satisfy the lifetime, alignment, and access rules in the C contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_session_process_event(
    session: *mut ImeSessionHandle,
    event: *const ImeInputEvent,
    out_result: *mut *mut ImeResult,
) -> u32 {
    ffi_boundary(|| {
        prepare_out(out_result)?;
        let event = parse_input_event(event)?;
        let session = require_mut(session)?;
        let result = session.inner.process_event(event).map_err(map_core_error)?;
        let owned = build_process_result(result, session.inner.reconciliation_required())?;
        store_owned_result(out_result, owned)
    })
}

/// Returns a full state snapshot for UI rebuild or lifecycle recovery.
///
/// # Safety
///
/// All non-null pointers must satisfy the lifetime, alignment, and access rules in the C contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_session_snapshot(
    session: *const ImeSessionHandle,
    out_result: *mut *mut ImeResult,
) -> u32 {
    ffi_boundary(|| {
        prepare_out(out_result)?;
        let session = require_ref(session)?;
        let owned = build_snapshot_result(&session.inner)?;
        store_owned_result(out_result, owned)
    })
}

/// Records the platform disposition of one action.
///
/// # Safety
///
/// `session` must be a live, exclusively accessed session handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_session_ack_action(
    session: *mut ImeSessionHandle,
    action_id: u64,
    disposition: u32,
) -> u32 {
    ffi_boundary(|| {
        let disposition = parse_action_disposition(disposition)?;
        let session = require_mut(session)?;
        session
            .inner
            .acknowledge_action(ActionId::new(action_id), disposition)
            .map_err(map_core_error)?;
        Ok(())
    })
}

/// Frees a result and every pointer reachable from it. A null result is a no-op.
///
/// # Safety
///
/// A non-null `result` must be a live value returned by this crate and not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ime_v1_result_free(result: *mut ImeResult) -> u32 {
    ffi_boundary(|| {
        if !result.is_null() {
            // SAFETY: `wire` is the first repr(C) field of the allocation returned by this crate.
            unsafe { drop(Box::from_raw(result.cast::<OwnedResult>())) };
        }
        Ok(())
    })
}

fn ffi_boundary(operation: impl FnOnce() -> Result<(), u32>) -> u32 {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => IME_STATUS_OK,
        Ok(Err(status)) => status,
        Err(_) => IME_STATUS_INTERNAL_PANIC,
    }
}

fn prepare_out<T>(out: *mut *mut T) -> Result<(), u32> {
    if out.is_null() {
        return Err(IME_STATUS_NULL_POINTER);
    }
    // SAFETY: a non-null out pointer must reference writable pointer storage for this call.
    unsafe { out.write(ptr::null_mut()) };
    Ok(())
}

fn store_out<T>(out: *mut *mut T, value: *mut T) -> Result<(), u32> {
    if out.is_null() {
        return Err(IME_STATUS_NULL_POINTER);
    }
    // SAFETY: `prepare_out` validated the caller-provided writable output slot.
    unsafe { out.write(value) };
    Ok(())
}

fn require_ref<'call, T>(value: *const T) -> Result<&'call T, u32> {
    if value.is_null() {
        return Err(IME_STATUS_NULL_POINTER);
    }
    // SAFETY: the ABI contract requires a live, correctly aligned handle for this call.
    Ok(unsafe { &*value })
}

fn require_mut<'call, T>(value: *mut T) -> Result<&'call mut T, u32> {
    if value.is_null() {
        return Err(IME_STATUS_NULL_POINTER);
    }
    // SAFETY: the ABI contract requires unique access to a live, aligned handle for this call.
    Ok(unsafe { &mut *value })
}

fn validate_abi_version(caller_version: u32, library_version: u32) -> Result<(), u32> {
    let caller_major = caller_version >> 16;
    let caller_minor = caller_version & 0xffff;
    let library_major = library_version >> 16;
    let library_minor = library_version & 0xffff;

    if caller_major != library_major || caller_minor > library_minor {
        return Err(IME_STATUS_UNSUPPORTED_ABI_VERSION);
    }
    Ok(())
}

fn validate_struct_size(supplied_size: u32, minimum_required_size: usize) -> Result<(), u32> {
    let supplied_size = usize::try_from(supplied_size).map_err(|_| IME_STATUS_INVALID_ARGUMENT)?;
    if supplied_size < minimum_required_size {
        return Err(IME_STATUS_INVALID_ARGUMENT);
    }
    Ok(())
}

fn minimum_input_event_size(caller_version: u32) -> Result<usize, u32> {
    match caller_version {
        0x0001_0000 => Ok(size_of::<ImeInputEvent>()),
        _ => Err(IME_STATUS_UNSUPPORTED_ABI_VERSION),
    }
}

fn parse_input_event(event: *const ImeInputEvent) -> Result<InputEvent, u32> {
    if event.is_null() {
        return Err(IME_STATUS_NULL_POINTER);
    }

    // SAFETY: the ABI requires the pointer to expose at least the first u32 field.
    let declared_size = unsafe { event.cast::<u32>().read_unaligned() };
    validate_struct_size(declared_size, size_of::<ImeStructHeader>())?;

    // SAFETY: the declared size covers the common header prefix at the event address.
    let header = unsafe { event.cast::<ImeStructHeader>().read_unaligned() };
    validate_abi_version(header.abi_version, IME_ABI_VERSION)?;
    validate_struct_size(
        header.struct_size,
        minimum_input_event_size(header.abi_version)?,
    )?;

    // SAFETY: the declared-version minimum covers all v1.0 fields read below.
    let wire = unsafe { event.read_unaligned() };

    match wire.tag {
        IME_EVENT_INSERT_TEXT => Ok(InputEvent::InsertText(read_input_string(wire.text)?)),
        IME_EVENT_BACKSPACE => Ok(InputEvent::Backspace),
        IME_EVENT_DELETE_FORWARD => Ok(InputEvent::DeleteForward),
        IME_EVENT_MOVE_COMPOSITION_CURSOR => Ok(InputEvent::MoveCompositionCursor {
            grapheme_delta: wire.delta,
        }),
        IME_EVENT_SELECT_CANDIDATE => Ok(InputEvent::SelectCandidate {
            candidate_id: CandidateId::new(wire.candidate_id),
            state_revision: StateRevision::new(wire.state_revision),
        }),
        IME_EVENT_MOVE_CANDIDATE_SELECTION => {
            Ok(InputEvent::MoveCandidateSelection { delta: wire.delta })
        }
        IME_EVENT_COMMIT => Ok(InputEvent::Commit),
        IME_EVENT_CANCEL => Ok(InputEvent::Cancel),
        IME_EVENT_RESET => Ok(InputEvent::Reset),
        _ => Err(IME_STATUS_UNSUPPORTED),
    }
}

fn read_input_string(view: ImeStringView) -> Result<String, u32> {
    if view.len == 0 {
        return Ok(String::new());
    }
    if view.ptr.is_null() {
        return Err(IME_STATUS_NULL_POINTER);
    }
    if view.len > MAX_FFI_INPUT_BYTES {
        return Err(IME_STATUS_BUDGET_EXCEEDED);
    }
    let length = usize::try_from(view.len).map_err(|_| IME_STATUS_BUDGET_EXCEEDED)?;
    // SAFETY: the caller guarantees `ptr` is readable for `len` bytes during this call.
    let bytes = unsafe { slice::from_raw_parts(view.ptr, length) };
    let text = std::str::from_utf8(bytes).map_err(|_| IME_STATUS_INVALID_UTF8)?;
    Ok(text.to_owned())
}

fn parse_action_disposition(value: u32) -> Result<ActionDisposition, u32> {
    match value {
        IME_ACTION_APPLIED => Ok(ActionDisposition::Applied),
        IME_ACTION_REJECTED => Ok(ActionDisposition::Rejected),
        IME_ACTION_UNAVAILABLE => Ok(ActionDisposition::Unavailable),
        IME_ACTION_SUPERSEDED => Ok(ActionDisposition::Superseded),
        IME_ACTION_FAILED => Ok(ActionDisposition::Failed),
        _ => Err(IME_STATUS_INVALID_ARGUMENT),
    }
}

fn build_process_result(
    result: CoreResult,
    reconciliation_required: bool,
) -> Result<Box<OwnedResult>, u32> {
    build_owned_result(
        ResultMetadata {
            status: map_event_status(result.status),
            event_handling: map_event_handling(result.event_handling),
            session_id: result.session_id.get(),
            state_revision: result.state_revision.get(),
            resource_generation: result.resource_generation.get(),
            reconciliation_required,
        },
        result.state,
        result.actions,
        result.degraded_components,
    )
}

fn build_snapshot_result(session: &ImeSession) -> Result<Box<OwnedResult>, u32> {
    build_owned_result(
        ResultMetadata {
            status: IME_EVENT_STATUS_OK,
            event_handling: IME_EVENT_HANDLING_CONSUMED,
            session_id: session.id().get(),
            state_revision: session.state_revision().get(),
            resource_generation: session.resource_generation().get(),
            reconciliation_required: session.reconciliation_required(),
        },
        session.snapshot(),
        Vec::new(),
        session.degraded_components().to_vec(),
    )
}

fn build_owned_result(
    metadata: ResultMetadata,
    state: ImeState,
    actions: Vec<CoreAction>,
    degraded_components: Vec<DegradedComponent>,
) -> Result<Box<OwnedResult>, u32> {
    if state.candidates.len() > MAX_RESULT_CANDIDATES || actions.len() > MAX_RESULT_ACTIONS {
        return Err(IME_STATUS_BUDGET_EXCEEDED);
    }
    let mut total_text_bytes = state.composition_utf8.len();
    for candidate in &state.candidates {
        total_text_bytes = total_text_bytes
            .checked_add(candidate.text.len())
            .ok_or(IME_STATUS_BUDGET_EXCEEDED)?;
    }
    for action in &actions {
        let text_len = match &action.kind {
            ImeActionKind::SetCompositionText { text_utf8, .. }
            | ImeActionKind::CommitText { text_utf8, .. } => text_utf8.len(),
            _ => 0,
        };
        total_text_bytes = total_text_bytes
            .checked_add(text_len)
            .ok_or(IME_STATUS_BUDGET_EXCEEDED)?;
    }
    if total_text_bytes > MAX_RESULT_TEXT_BYTES {
        return Err(IME_STATUS_BUDGET_EXCEEDED);
    }

    let composition = state.composition_utf8.into_bytes();
    let mut candidate_texts = Vec::with_capacity(state.candidates.len());
    let mut candidate_wires = Vec::with_capacity(state.candidates.len());
    for candidate in state.candidates {
        candidate_texts.push(candidate.text.into_bytes());
        candidate_wires.push(ImeCandidate {
            candidate_id: candidate.candidate_id.get(),
            reserved: 0,
            text: string_view(candidate_texts.last().expect("just pushed candidate text")),
        });
    }

    let mut action_texts = Vec::with_capacity(actions.len());
    let mut action_wires = Vec::with_capacity(actions.len());
    for action in actions {
        let kind = map_action_kind(action.kind)?;
        action_texts.push(kind.text);
        action_wires.push(ImeAction {
            struct_size: u32::try_from(size_of::<ImeAction>())
                .map_err(|_| IME_STATUS_INTERNAL_ERROR)?,
            abi_version: IME_ABI_VERSION,
            action_id: action.action_id.get(),
            session_id: action.session_id.get(),
            originating_revision: action.originating_revision.get(),
            tag: kind.tag,
            commit_kind: kind.commit_kind,
            text: string_view(action_texts.last().expect("just pushed action text")),
            cursor_utf8_byte_offset: kind.cursor_utf8_byte_offset,
            operation_count: kind.operation_count,
            reset_reason: kind.reset_reason,
        });
    }

    let candidates = candidate_wires.into_boxed_slice();
    let actions = action_wires.into_boxed_slice();
    let degraded_components: Box<[u32]> = degraded_components
        .into_iter()
        .map(map_degraded_component)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let result_flags: Box<[u32]> = Box::default();
    let (has_selected_candidate, selected_candidate_id) = state
        .selected_candidate
        .map_or((0, 0), |candidate_id| (1, candidate_id.get()));

    let mut owned = Box::new(OwnedResult {
        wire: ImeResult {
            struct_size: u32::try_from(size_of::<ImeResult>())
                .map_err(|_| IME_STATUS_INTERNAL_ERROR)?,
            abi_version: IME_ABI_VERSION,
            status: metadata.status,
            event_handling: metadata.event_handling,
            session_id: metadata.session_id,
            state_revision: metadata.state_revision,
            resource_generation: metadata.resource_generation,
            phase: map_phase(state.phase),
            reserved: 0,
            composition: ImeStringView {
                ptr: ptr::null(),
                len: 0,
            },
            composition_cursor_grapheme: u64::try_from(state.composition_cursor_grapheme)
                .map_err(|_| IME_STATUS_BUDGET_EXCEEDED)?,
            composition_cursor_utf8_byte_offset: u64::try_from(
                state.composition_cursor_utf8_byte_offset,
            )
            .map_err(|_| IME_STATUS_BUDGET_EXCEEDED)?,
            candidates: ptr::null(),
            candidate_count: u64::try_from(candidates.len())
                .map_err(|_| IME_STATUS_BUDGET_EXCEEDED)?,
            has_selected_candidate,
            selected_candidate_id,
            actions: ptr::null(),
            action_count: u64::try_from(actions.len()).map_err(|_| IME_STATUS_BUDGET_EXCEEDED)?,
            reconciliation_required: u32::from(metadata.reconciliation_required),
            reserved2: 0,
            degraded_components: ptr::null(),
            degraded_component_count: u64::try_from(degraded_components.len())
                .map_err(|_| IME_STATUS_BUDGET_EXCEEDED)?,
            result_flags: ptr::null(),
            result_flag_count: 0,
        },
        _composition: composition,
        _candidate_texts: candidate_texts,
        _candidates: candidates,
        _action_texts: action_texts,
        _actions: actions,
        _degraded_components: degraded_components,
        _result_flags: result_flags,
    });
    owned.wire.composition = string_view(&owned._composition);
    owned.wire.candidates = slice_pointer(&owned._candidates);
    owned.wire.actions = slice_pointer(&owned._actions);
    owned.wire.degraded_components = slice_pointer(&owned._degraded_components);
    owned.wire.result_flags = slice_pointer(&owned._result_flags);
    Ok(owned)
}

fn map_action_kind(kind: ImeActionKind) -> Result<WireActionKind, u32> {
    match kind {
        ImeActionKind::SetCompositionText {
            text_utf8,
            cursor_utf8_byte_offset,
        } => Ok(WireActionKind {
            tag: IME_ACTION_SET_COMPOSITION_TEXT,
            commit_kind: 0,
            text: text_utf8.into_bytes(),
            cursor_utf8_byte_offset: u64::try_from(cursor_utf8_byte_offset)
                .map_err(|_| IME_STATUS_BUDGET_EXCEEDED)?,
            operation_count: 0,
            reset_reason: 0,
        }),
        ImeActionKind::FinishComposition => Ok(WireActionKind {
            tag: IME_ACTION_FINISH_COMPOSITION,
            commit_kind: 0,
            text: Vec::new(),
            cursor_utf8_byte_offset: 0,
            operation_count: 0,
            reset_reason: 0,
        }),
        ImeActionKind::CancelComposition => Ok(WireActionKind {
            tag: IME_ACTION_CANCEL_COMPOSITION,
            commit_kind: 0,
            text: Vec::new(),
            cursor_utf8_byte_offset: 0,
            operation_count: 0,
            reset_reason: 0,
        }),
        ImeActionKind::CommitText {
            text_utf8,
            commit_kind,
        } => Ok(WireActionKind {
            tag: IME_ACTION_COMMIT_TEXT,
            commit_kind: map_commit_kind(commit_kind),
            text: text_utf8.into_bytes(),
            cursor_utf8_byte_offset: 0,
            operation_count: 0,
            reset_reason: 0,
        }),
        ImeActionKind::DeleteBackward { operation_count } => Ok(WireActionKind {
            tag: IME_ACTION_DELETE_BACKWARD,
            commit_kind: 0,
            text: Vec::new(),
            cursor_utf8_byte_offset: 0,
            operation_count: u32::from(operation_count),
            reset_reason: 0,
        }),
        ImeActionKind::RequestPlatformReset { reason } => Ok(WireActionKind {
            tag: IME_ACTION_REQUEST_PLATFORM_RESET,
            commit_kind: 0,
            text: Vec::new(),
            cursor_utf8_byte_offset: 0,
            operation_count: 0,
            reset_reason: map_reset_reason(reason),
        }),
    }
}

fn string_view(bytes: &[u8]) -> ImeStringView {
    if bytes.is_empty() {
        ImeStringView {
            ptr: ptr::null(),
            len: 0,
        }
    } else {
        ImeStringView {
            ptr: bytes.as_ptr(),
            len: bytes.len() as u64,
        }
    }
}

fn slice_pointer<T>(values: &[T]) -> *const T {
    if values.is_empty() {
        ptr::null()
    } else {
        values.as_ptr()
    }
}

fn store_owned_result(out_result: *mut *mut ImeResult, owned: Box<OwnedResult>) -> Result<(), u32> {
    let raw = Box::into_raw(owned);
    // SAFETY: `wire` is the first repr(C) field and the allocation remains owned by the caller.
    let wire = unsafe { ptr::addr_of_mut!((*raw).wire) };
    store_out(out_result, wire)
}

fn map_event_status(status: CoreEventStatus) -> u32 {
    match status {
        CoreEventStatus::Ok => IME_EVENT_STATUS_OK,
        CoreEventStatus::NoOp => IME_EVENT_STATUS_NO_OP,
        CoreEventStatus::Unsupported => IME_EVENT_STATUS_UNSUPPORTED,
    }
}

fn map_event_handling(handling: EventHandling) -> u32 {
    match handling {
        EventHandling::Consumed => IME_EVENT_HANDLING_CONSUMED,
        EventHandling::PassThrough => IME_EVENT_HANDLING_PASS_THROUGH,
    }
}

fn map_phase(phase: SessionPhase) -> u32 {
    match phase {
        SessionPhase::Idle => IME_PHASE_IDLE,
        SessionPhase::Composing => IME_PHASE_COMPOSING,
        SessionPhase::CandidateSelecting => IME_PHASE_CANDIDATE_SELECTING,
    }
}

fn map_commit_kind(kind: CommitKind) -> u32 {
    match kind {
        CommitKind::Candidate => IME_COMMIT_CANDIDATE,
        CommitKind::RawFallback => IME_COMMIT_RAW_FALLBACK,
        CommitKind::DirectInput => IME_COMMIT_DIRECT_INPUT,
    }
}

fn map_reset_reason(reason: PlatformResetReason) -> u32 {
    match reason {
        PlatformResetReason::ExplicitReset => IME_RESET_EXPLICIT,
        PlatformResetReason::StateMismatch => IME_RESET_STATE_MISMATCH,
    }
}

fn map_degraded_component(component: DegradedComponent) -> u32 {
    match component {
        DegradedComponent::Language => IME_DEGRADED_LANGUAGE,
        DegradedComponent::Dictionary => IME_DEGRADED_DICTIONARY,
        DegradedComponent::Candidate => IME_DEGRADED_CANDIDATE,
        DegradedComponent::Ranking => IME_DEGRADED_RANKING,
    }
}

fn map_core_error(error: ImeError) -> u32 {
    match error {
        ImeError::InvalidConfig(_) => IME_STATUS_INVALID_ARGUMENT,
        ImeError::EventTextTooLong { .. }
        | ImeError::CompositionTooLong { .. }
        | ImeError::ContextTextTooLong { .. }
        | ImeError::TooManyOutstandingActions { .. } => IME_STATUS_BUDGET_EXCEEDED,
        ImeError::UnknownAction(_) => IME_STATUS_UNKNOWN_ACTION,
        ImeError::StaleRevision { .. } => IME_STATUS_STALE_REVISION,
        ImeError::UnknownCandidate(_) => IME_STATUS_UNKNOWN_CANDIDATE,
        ImeError::CounterExhausted(_) => IME_STATUS_INTERNAL_ERROR,
    }
}

fn reference_dictionary() -> InMemoryDictionary {
    InMemoryDictionary::new([
        DictionaryEntry::new(LexemeId::new(1), "ni", "你", 1000),
        DictionaryEntry::new(LexemeId::new(2), "ni", "尼", 300),
        DictionaryEntry::new(LexemeId::new(3), "ni", "呢", 200),
        DictionaryEntry::new(LexemeId::new(4), "nih", "你好", 100),
        DictionaryEntry::new(LexemeId::new(5), "nihao", "你好", 3000),
        DictionaryEntry::new(LexemeId::new(6), "nihao", "你号", 100),
        DictionaryEntry::new(LexemeId::new(7), "hao", "好", 2000),
        DictionaryEntry::new(LexemeId::new(8), "hao", "号", 800),
        DictionaryEntry::new(LexemeId::new(9), "zhong", "中", 2000),
        DictionaryEntry::new(LexemeId::new(10), "zhongguo", "中国", 5000),
        DictionaryEntry::new(LexemeId::new(11), "zhongguo", "中国", 4500),
        DictionaryEntry::new(LexemeId::new(12), "wo", "我", 4000),
    ])
}

#[cfg(test)]
mod tests {
    use std::{
        mem::{align_of, offset_of, size_of},
        ptr,
    };

    use super::*;

    fn ime_v1_engine_create(out_engine: *mut *mut ImeEngineHandle) -> u32 {
        // SAFETY: tests pass either contract-valid storage or an intentional null argument.
        unsafe { super::ime_v1_engine_create(out_engine) }
    }

    fn ime_v1_engine_destroy(engine: *mut ImeEngineHandle) -> u32 {
        // SAFETY: tests destroy each live test handle once; null is permitted by the contract.
        unsafe { super::ime_v1_engine_destroy(engine) }
    }

    fn ime_v1_session_create(
        engine: *const ImeEngineHandle,
        out_session: *mut *mut ImeSessionHandle,
    ) -> u32 {
        // SAFETY: tests pass live handles/output storage or intentional null arguments.
        unsafe { super::ime_v1_session_create(engine, out_session) }
    }

    fn ime_v1_session_destroy(session: *mut ImeSessionHandle) -> u32 {
        // SAFETY: tests destroy each live test handle once; null is permitted by the contract.
        unsafe { super::ime_v1_session_destroy(session) }
    }

    fn ime_v1_session_process_event(
        session: *mut ImeSessionHandle,
        event: *const ImeInputEvent,
        out_result: *mut *mut ImeResult,
    ) -> u32 {
        // SAFETY: tests keep every supplied object alive for the call or intentionally pass null.
        unsafe { super::ime_v1_session_process_event(session, event, out_result) }
    }

    fn ime_v1_session_snapshot(
        session: *const ImeSessionHandle,
        out_result: *mut *mut ImeResult,
    ) -> u32 {
        // SAFETY: tests pass a live session and writable output storage.
        unsafe { super::ime_v1_session_snapshot(session, out_result) }
    }

    fn ime_v1_session_ack_action(
        session: *mut ImeSessionHandle,
        action_id: u64,
        disposition: u32,
    ) -> u32 {
        // SAFETY: tests pass a live, exclusively accessed session.
        unsafe { super::ime_v1_session_ack_action(session, action_id, disposition) }
    }

    fn ime_v1_result_free(result: *mut ImeResult) -> u32 {
        // SAFETY: tests free each live result once; null is permitted by the contract.
        unsafe { super::ime_v1_result_free(result) }
    }

    fn base_event(tag: u32) -> ImeInputEvent {
        ImeInputEvent {
            struct_size: size_of::<ImeInputEvent>() as u32,
            abi_version: IME_ABI_VERSION,
            tag,
            reserved: 0,
            text: ImeStringView {
                ptr: ptr::null(),
                len: 0,
            },
            delta: 0,
            candidate_id: 0,
            state_revision: 0,
        }
    }

    fn create_handles() -> (*mut ImeEngineHandle, *mut ImeSessionHandle) {
        let mut engine = ptr::null_mut();
        assert_eq!(ime_v1_engine_create(&mut engine), IME_STATUS_OK);
        let mut session = ptr::null_mut();
        assert_eq!(ime_v1_session_create(engine, &mut session), IME_STATUS_OK);
        (engine, session)
    }

    fn process(session: *mut ImeSessionHandle, event: &ImeInputEvent) -> (*mut ImeResult, u32) {
        let mut result = ptr::null_mut();
        let status = ime_v1_session_process_event(session, event, &mut result);
        (result, status)
    }

    fn view_bytes(view: ImeStringView) -> Vec<u8> {
        if view.len == 0 {
            return Vec::new();
        }
        // SAFETY: tests only read views while their owning result is alive.
        unsafe { slice::from_raw_parts(view.ptr, view.len as usize).to_vec() }
    }

    #[test]
    fn abi_version_and_linux_layout_are_stable_for_this_target() {
        assert_eq!(ime_v1_get_abi_version(), 0x0001_0000);
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            assert_eq!(size_of::<ImeStructHeader>(), 8);
            assert_eq!(align_of::<ImeStructHeader>(), 4);
            assert_eq!(offset_of!(ImeStructHeader, struct_size), 0);
            assert_eq!(offset_of!(ImeStructHeader, abi_version), 4);
            assert_eq!(size_of::<ImeStringView>(), 16);
            assert_eq!(align_of::<ImeStringView>(), 8);
            assert_eq!(size_of::<ImeInputEvent>(), 48);
            assert_eq!(align_of::<ImeInputEvent>(), 8);
            assert_eq!(offset_of!(ImeInputEvent, text), 16);
            assert_eq!(offset_of!(ImeInputEvent, state_revision), 40);
            assert_eq!(size_of::<ImeCandidate>(), 24);
            assert_eq!(size_of::<ImeAction>(), 72);
            assert_eq!(offset_of!(ImeAction, text), 40);
            assert_eq!(size_of::<ImeResult>(), 160);
            assert_eq!(offset_of!(ImeResult, composition), 48);
            assert_eq!(offset_of!(ImeResult, actions), 104);
        }
    }

    #[test]
    fn abi_version_compatibility_is_directional() {
        assert_eq!(validate_abi_version(0x0001_0000, 0x0001_0000), Ok(()));
        assert_eq!(
            validate_abi_version(0x0001_0001, 0x0001_0000),
            Err(IME_STATUS_UNSUPPORTED_ABI_VERSION)
        );
        assert_eq!(
            validate_abi_version(0x0001_ffff, 0x0001_0000),
            Err(IME_STATUS_UNSUPPORTED_ABI_VERSION)
        );
        assert_eq!(
            validate_abi_version(0x0002_0000, 0x0001_0000),
            Err(IME_STATUS_UNSUPPORTED_ABI_VERSION)
        );

        let synthetic_library_v1_1 = 0x0001_0001;
        assert_eq!(
            validate_abi_version(0x0001_0000, synthetic_library_v1_1),
            Ok(())
        );
        assert_eq!(
            validate_abi_version(0x0001_0001, synthetic_library_v1_1),
            Ok(())
        );
        assert_eq!(
            validate_abi_version(0x0001_0002, synthetic_library_v1_1),
            Err(IME_STATUS_UNSUPPORTED_ABI_VERSION)
        );
        assert_eq!(
            validate_abi_version(0x0002_0000, synthetic_library_v1_1),
            Err(IME_STATUS_UNSUPPORTED_ABI_VERSION)
        );
    }

    #[test]
    fn input_event_size_uses_declared_version_prefix() {
        #[repr(C)]
        struct ExtendedInputEvent {
            event: ImeInputEvent,
            trailing_bytes: [u8; 16],
        }

        let (engine, session) = create_handles();

        let exact = base_event(IME_EVENT_BACKSPACE);
        let (exact_result, status) = process(session, &exact);
        assert_eq!(status, IME_STATUS_OK);
        assert_eq!(ime_v1_result_free(exact_result), IME_STATUS_OK);

        let mut extended = ExtendedInputEvent {
            event: base_event(IME_EVENT_BACKSPACE),
            trailing_bytes: [0xa5; 16],
        };
        extended.event.struct_size = size_of::<ExtendedInputEvent>() as u32;
        let (extended_result, status) = process(session, &extended.event);
        assert_eq!(status, IME_STATUS_OK);
        assert_eq!(ime_v1_result_free(extended_result), IME_STATUS_OK);
        assert_eq!(extended.trailing_bytes, [0xa5; 16]);

        let mut undersized = base_event(IME_EVENT_BACKSPACE);
        undersized.struct_size = size_of::<ImeInputEvent>() as u32 - 1;
        let (result, status) = process(session, &undersized);
        assert_eq!(status, IME_STATUS_INVALID_ARGUMENT);
        assert!(result.is_null());

        for future_version in [0x0001_0001, 0x0001_ffff, 0x0002_0000] {
            extended.event.abi_version = future_version;
            let (result, status) = process(session, &extended.event);
            assert_eq!(status, IME_STATUS_UNSUPPORTED_ABI_VERSION);
            assert!(result.is_null());
        }

        assert_eq!(ime_v1_session_destroy(session), IME_STATUS_OK);
        assert_eq!(ime_v1_engine_destroy(engine), IME_STATUS_OK);
    }

    #[test]
    fn engine_can_be_destroyed_before_live_session() {
        let (engine, session) = create_handles();
        assert_eq!(ime_v1_engine_destroy(engine), IME_STATUS_OK);

        let text = b"nihao";
        let mut event = base_event(IME_EVENT_INSERT_TEXT);
        event.text = ImeStringView {
            ptr: text.as_ptr(),
            len: text.len() as u64,
        };
        let (result, status) = process(session, &event);
        assert_eq!(status, IME_STATUS_OK);
        assert!(!result.is_null());
        assert_eq!(ime_v1_result_free(result), IME_STATUS_OK);
        assert_eq!(ime_v1_session_destroy(session), IME_STATUS_OK);
    }

    #[test]
    fn utf8_views_support_embedded_nul_and_reject_invalid_utf8() {
        let (engine, session) = create_handles();
        let bytes = b"a\0b";
        let mut event = base_event(IME_EVENT_INSERT_TEXT);
        event.text = ImeStringView {
            ptr: bytes.as_ptr(),
            len: bytes.len() as u64,
        };
        let (result, status) = process(session, &event);
        assert_eq!(status, IME_STATUS_OK);
        // SAFETY: result is non-null on successful processing and remains owned here.
        assert_eq!(view_bytes(unsafe { (*result).composition }), bytes);
        assert_eq!(ime_v1_result_free(result), IME_STATUS_OK);

        let invalid = [0xff];
        let mut invalid_event = base_event(IME_EVENT_INSERT_TEXT);
        invalid_event.text = ImeStringView {
            ptr: invalid.as_ptr(),
            len: 1,
        };
        let (result, status) = process(session, &invalid_event);
        assert_eq!(status, IME_STATUS_INVALID_UTF8);
        assert!(result.is_null());

        assert_eq!(ime_v1_session_destroy(session), IME_STATUS_OK);
        assert_eq!(ime_v1_engine_destroy(engine), IME_STATUS_OK);
    }

    #[test]
    fn invalid_inputs_return_structured_statuses() {
        let (engine, session) = create_handles();
        let mut out = ptr::null_mut();
        assert_eq!(
            ime_v1_session_process_event(ptr::null_mut(), ptr::null(), &mut out),
            IME_STATUS_NULL_POINTER
        );
        let event = base_event(IME_EVENT_BACKSPACE);
        assert_eq!(
            ime_v1_session_process_event(session, &event, ptr::null_mut()),
            IME_STATUS_NULL_POINTER
        );

        let mut null_text = base_event(IME_EVENT_INSERT_TEXT);
        null_text.text.len = 1;
        let (_, status) = process(session, &null_text);
        assert_eq!(status, IME_STATUS_NULL_POINTER);

        let mut unknown = base_event(999);
        let (_, status) = process(session, &unknown);
        assert_eq!(status, IME_STATUS_UNSUPPORTED);

        unknown.tag = IME_EVENT_BACKSPACE;
        unknown.struct_size = 8;
        let (_, status) = process(session, &unknown);
        assert_eq!(status, IME_STATUS_INVALID_ARGUMENT);

        unknown.struct_size = size_of::<ImeInputEvent>() as u32;
        unknown.abi_version = 0x0002_0000;
        let (_, status) = process(session, &unknown);
        assert_eq!(status, IME_STATUS_UNSUPPORTED_ABI_VERSION);

        unknown.struct_size = size_of::<ImeInputEvent>() as u32 + 16;
        unknown.abi_version = IME_ABI_VERSION + 1;
        let (future_result, status) = process(session, &unknown);
        assert_eq!(status, IME_STATUS_UNSUPPORTED_ABI_VERSION);
        assert!(future_result.is_null());

        let oversized = vec![b'a'; MAX_FFI_INPUT_BYTES as usize + 1];
        let mut oversized_event = base_event(IME_EVENT_INSERT_TEXT);
        oversized_event.text = ImeStringView {
            ptr: oversized.as_ptr(),
            len: oversized.len() as u64,
        };
        let (_, status) = process(session, &oversized_event);
        assert_eq!(status, IME_STATUS_BUDGET_EXCEEDED);

        assert_eq!(
            ime_v1_session_ack_action(session, 1, 999),
            IME_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            ime_v1_session_create(ptr::null(), &mut ptr::null_mut()),
            IME_STATUS_NULL_POINTER
        );
        assert_eq!(ime_v1_session_destroy(session), IME_STATUS_OK);
        assert_eq!(ime_v1_engine_destroy(engine), IME_STATUS_OK);
    }

    #[test]
    fn candidate_snapshot_commit_and_failed_recovery_cross_abi() {
        let (engine, session) = create_handles();
        let text = b"nihao";
        let mut insert = base_event(IME_EVENT_INSERT_TEXT);
        insert.text = ImeStringView {
            ptr: text.as_ptr(),
            len: text.len() as u64,
        };
        let (insert_result, status) = process(session, &insert);
        assert_eq!(status, IME_STATUS_OK);
        // SAFETY: the successful result remains alive until freed below.
        let insert_wire = unsafe { &*insert_result };
        assert_eq!(insert_wire.phase, IME_PHASE_CANDIDATE_SELECTING);
        assert_eq!(view_bytes(insert_wire.composition), b"nihao");
        assert!(insert_wire.candidate_count >= 2);
        // SAFETY: candidate_count is non-zero and the array belongs to the live result.
        let first = unsafe { &*insert_wire.candidates };
        assert_eq!(view_bytes(first.text), "你好".as_bytes());
        let revision = insert_wire.state_revision;
        assert_eq!(ime_v1_result_free(insert_result), IME_STATUS_OK);

        let mut select = base_event(IME_EVENT_SELECT_CANDIDATE);
        select.candidate_id = 1;
        select.state_revision = revision;
        let (commit_result, status) = process(session, &select);
        assert_eq!(status, IME_STATUS_OK);
        // SAFETY: the successful result owns one action until it is freed.
        let commit_wire = unsafe { &*commit_result };
        assert_eq!(commit_wire.action_count, 1);
        // SAFETY: action_count is one and the array belongs to the live result.
        let action = unsafe { &*commit_wire.actions };
        assert_eq!(action.tag, IME_ACTION_COMMIT_TEXT);
        assert_eq!(action.commit_kind, IME_COMMIT_CANDIDATE);
        assert_eq!(view_bytes(action.text), "你好".as_bytes());
        let action_id = action.action_id;
        assert_eq!(ime_v1_result_free(commit_result), IME_STATUS_OK);

        assert_eq!(
            ime_v1_session_ack_action(session, action_id, IME_ACTION_FAILED),
            IME_STATUS_OK
        );
        let mut snapshot = ptr::null_mut();
        assert_eq!(
            ime_v1_session_snapshot(session, &mut snapshot),
            IME_STATUS_OK
        );
        // SAFETY: snapshot is non-null and owned until result_free.
        let snapshot_wire = unsafe { &*snapshot };
        assert_eq!(snapshot_wire.phase, IME_PHASE_CANDIDATE_SELECTING);
        assert_eq!(view_bytes(snapshot_wire.composition), b"nihao");
        assert!(snapshot_wire.state_revision > revision);
        assert!(snapshot_wire.candidate_count >= 2);
        assert_eq!(ime_v1_result_free(snapshot), IME_STATUS_OK);

        assert_eq!(ime_v1_session_destroy(session), IME_STATUS_OK);
        assert_eq!(ime_v1_engine_destroy(engine), IME_STATUS_OK);
    }

    #[test]
    fn stale_revision_and_invalid_candidate_are_distinct() {
        let (engine, session) = create_handles();
        let text = b"nihao";
        let mut insert = base_event(IME_EVENT_INSERT_TEXT);
        insert.text = ImeStringView {
            ptr: text.as_ptr(),
            len: text.len() as u64,
        };
        let (result, status) = process(session, &insert);
        assert_eq!(status, IME_STATUS_OK);
        // SAFETY: result is live and successful.
        let revision = unsafe { (*result).state_revision };
        assert_eq!(ime_v1_result_free(result), IME_STATUS_OK);

        let mut invalid = base_event(IME_EVENT_SELECT_CANDIDATE);
        invalid.candidate_id = 999;
        invalid.state_revision = revision;
        let (_, status) = process(session, &invalid);
        assert_eq!(status, IME_STATUS_UNKNOWN_CANDIDATE);

        let mut movement = base_event(IME_EVENT_MOVE_CANDIDATE_SELECTION);
        movement.delta = 1;
        let (movement_result, status) = process(session, &movement);
        assert_eq!(status, IME_STATUS_OK);
        assert_eq!(ime_v1_result_free(movement_result), IME_STATUS_OK);
        invalid.candidate_id = 1;
        let (_, status) = process(session, &invalid);
        assert_eq!(status, IME_STATUS_STALE_REVISION);

        assert_eq!(ime_v1_session_destroy(session), IME_STATUS_OK);
        assert_eq!(ime_v1_engine_destroy(engine), IME_STATUS_OK);
    }

    #[test]
    fn null_zero_length_view_is_empty() {
        let (engine, session) = create_handles();
        let event = base_event(IME_EVENT_INSERT_TEXT);
        let (result, status) = process(session, &event);
        assert_eq!(status, IME_STATUS_OK);
        assert!(!result.is_null());
        assert_eq!(ime_v1_result_free(result), IME_STATUS_OK);
        assert_eq!(ime_v1_session_destroy(session), IME_STATUS_OK);
        assert_eq!(ime_v1_engine_destroy(engine), IME_STATUS_OK);
    }

    #[test]
    fn panic_boundary_maps_catchable_unwind() {
        let status = ffi_boundary(|| -> Result<(), u32> { panic!("injected test panic") });
        assert_eq!(status, IME_STATUS_INTERNAL_PANIC);
    }
}
