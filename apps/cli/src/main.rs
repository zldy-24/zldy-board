use std::io::{self, Write};

use ime_core::{
    ActionDisposition, ActionId, EngineConfig, ImeAction, ImeEngine, ImeResult, ImeSession,
    InputEvent,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = ImeEngine::new(EngineConfig::default())?;
    let mut session = engine.new_session()?;
    let mut auto_ack = true;

    println!("ime-cli — Phase 1A.1 Core REPL");
    println!(
        "commands: text <TEXT>, backspace, delete, left, right, commit, cancel, state, reset, \
         autoack on|off, ack <ID> applied|failed|rejected|unavailable|superseded, quit"
    );

    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);

        if line == "quit" {
            break;
        }
        if line == "state" {
            print_state(&session);
            continue;
        }
        if let Some(value) = line.strip_prefix("autoack ") {
            match value {
                "on" => {
                    auto_ack = true;
                    println!("autoack: on");
                }
                "off" => {
                    auto_ack = false;
                    println!("autoack: off");
                }
                _ => println!("usage: autoack on|off"),
            }
            continue;
        }
        if let Some(arguments) = line.strip_prefix("ack ") {
            acknowledge_one(&mut session, arguments);
            continue;
        }

        let event = if let Some(text) = line.strip_prefix("text ") {
            InputEvent::InsertText(text.to_owned())
        } else {
            match line {
                "backspace" => InputEvent::Backspace,
                "delete" => InputEvent::DeleteForward,
                "left" => InputEvent::MoveCompositionCursor { grapheme_delta: -1 },
                "right" => InputEvent::MoveCompositionCursor { grapheme_delta: 1 },
                "commit" => InputEvent::Commit,
                "cancel" => InputEvent::Cancel,
                "reset" => InputEvent::Reset,
                "" => continue,
                _ => {
                    println!("unknown command: {line}");
                    continue;
                }
            }
        };

        match session.process_event(event) {
            Ok(result) => {
                print_result(&result);
                if auto_ack {
                    acknowledge_actions(&mut session, &result.actions);
                } else {
                    println!("pending actions: {}", session.outstanding_action_count());
                }
            }
            Err(error) => println!("error: {error}"),
        }
    }

    Ok(())
}

fn print_result(result: &ImeResult) {
    println!("revision: {}", result.state_revision.get());
    println!("handling: {:?}", result.event_handling);
    println!("status: {:?}", result.status);
    println!("phase: {:?}", result.state.phase);
    println!("composition: {:?}", result.state.composition_utf8);
    println!(
        "cursor: {} grapheme(s), byte {}",
        result.state.composition_cursor_grapheme, result.state.composition_cursor_utf8_byte_offset
    );
    if result.actions.is_empty() {
        println!("actions: []");
    } else {
        for action in &result.actions {
            println!("action {}: {:?}", action.action_id.get(), action.kind);
        }
    }
}

fn print_state(session: &ImeSession) {
    let state = session.snapshot();
    println!("revision: {}", session.state_revision().get());
    println!("phase: {:?}", state.phase);
    println!("composition: {:?}", state.composition_utf8);
    println!(
        "cursor: {} grapheme(s), byte {}",
        state.composition_cursor_grapheme, state.composition_cursor_utf8_byte_offset
    );
    println!(
        "outstanding actions: {}",
        session.outstanding_action_count()
    );
    println!(
        "reconciliation required: {}",
        session.reconciliation_required()
    );
}

fn acknowledge_actions(session: &mut ImeSession, actions: &[ImeAction]) {
    for action in actions {
        match session.acknowledge_action(action.action_id, ActionDisposition::Applied) {
            Ok(outcome) => println!("ack: {outcome:?}"),
            Err(error) => println!("ack error: {error}"),
        }
    }
}

fn acknowledge_one(session: &mut ImeSession, arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let Some(action_id) = parts.next() else {
        println!("usage: ack <ID> applied|failed|rejected|unavailable|superseded");
        return;
    };
    let Some(disposition) = parts.next() else {
        println!("usage: ack <ID> applied|failed|rejected|unavailable|superseded");
        return;
    };
    if parts.next().is_some() {
        println!("usage: ack <ID> applied|failed|rejected|unavailable|superseded");
        return;
    }

    let Ok(action_id) = action_id.parse::<u64>() else {
        println!("invalid action id: {action_id}");
        return;
    };
    let Some(disposition) = parse_disposition(disposition) else {
        println!("invalid disposition: {disposition}");
        return;
    };

    match session.acknowledge_action(ActionId::new(action_id), disposition) {
        Ok(outcome) => {
            println!("ack: {outcome:?}");
            print_state(session);
        }
        Err(error) => println!("ack error: {error}"),
    }
}

fn parse_disposition(value: &str) -> Option<ActionDisposition> {
    match value {
        "applied" => Some(ActionDisposition::Applied),
        "failed" => Some(ActionDisposition::Failed),
        "rejected" => Some(ActionDisposition::Rejected),
        "unavailable" => Some(ActionDisposition::Unavailable),
        "superseded" => Some(ActionDisposition::Superseded),
        _ => None,
    }
}
