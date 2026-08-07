use crate::resp::parser::RespTypes;

/// Turns a frame the client sent into the frame we should send back.
///
/// Clients always send an array of bulk strings: element 0 is the command name,
/// the rest are its arguments. Anything else is a client mistake, and a client
/// mistake is a normal `-ERR` reply — not a `Result`, and never a dropped
/// connection.
pub fn dispatch(frame: RespTypes) -> RespTypes {
    // `mut` because we hand arguments back by moving them out of the vec below.
    let parts = match frame {
        RespTypes::Array(Some(parts)) => {
            if !parts.is_empty() {
                parts
            } else {
                return error("Empty");
            }
        }
        _ => {
            return error("Resp always sends array as input");
        }
    };

    // Every element has to be a real bulk string before we can look at any of
    // them, so validate the whole array up front rather than mid-dispatch.
    let mut args: Vec<String> = Vec::new();
    for part in parts.iter() {
        match part {
            RespTypes::BulkString(Some(bytes)) => match bulk_to_string(bytes) {
                Some(text) => args.push(text),
                None => return error("Command names and arguments must be valid UTF-8"),
            },
            _ => return error("Every element must be a bulk string"),
        }
    }

    // Command names are case-insensitive: `ping` and `PING` are the same command.
    // `remove(0)` takes the name out, so element 0 is now the first argument.
    let name = args.remove(0).to_uppercase();

    match name.as_str() {
        "PING" => match args.len() {
            0 => RespTypes::SimpleString("PONG".to_string()),
            1 => RespTypes::BulkString(Some(args.remove(0).into_bytes())),
            _ => error("wrong number of arguments for 'ping' command"),
        },
        "ECHO" => match args.len() {
            1 => RespTypes::BulkString(Some(args.remove(0).into_bytes())),
            _ => error("wrong number of arguments for 'echo' command"),
        },
        _ => error(&format!("unknown command '{name}'")),
    }
}

/// Pulls the text out of a bulk string.
///
/// `None` if the frame is not a bulk string, is the null bulk string, or holds
/// bytes that are not valid UTF-8. Command names are always text, so that is a
/// client error rather than something to pass along.
fn bulk_to_string(frame: &[u8]) -> Option<String> {
    String::from_utf8(frame.to_vec()).ok()
}

fn error(message: &str) -> RespTypes {
    RespTypes::Error(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the frame a real client would send: an array of bulk strings.
    fn command(parts: &[&str]) -> RespTypes {
        RespTypes::Array(Some(
            parts
                .iter()
                .map(|p| RespTypes::BulkString(Some(p.as_bytes().to_vec())))
                .collect(),
        ))
    }

    fn bulk(s: &str) -> RespTypes {
        RespTypes::BulkString(Some(s.as_bytes().to_vec()))
    }

    #[test]
    fn ping_with_no_arguments_replies_pong() {
        assert_eq!(
            dispatch(command(&["PING"])),
            RespTypes::SimpleString("PONG".to_string())
        );
    }

    #[test]
    fn ping_with_one_argument_replies_with_that_argument() {
        assert_eq!(dispatch(command(&["PING", "hello"])), bulk("hello"));
    }

    #[test]
    fn echo_replies_with_its_argument() {
        assert_eq!(dispatch(command(&["ECHO", "hello"])), bulk("hello"));
    }

    #[test]
    fn command_names_are_case_insensitive() {
        assert_eq!(
            dispatch(command(&["ping"])),
            RespTypes::SimpleString("PONG".to_string())
        );
        assert_eq!(dispatch(command(&["EcHo", "hi"])), bulk("hi"));
    }

    #[test]
    fn echo_without_an_argument_is_an_error() {
        assert!(matches!(dispatch(command(&["ECHO"])), RespTypes::Error(_)));
    }

    #[test]
    fn echo_with_too_many_arguments_is_an_error() {
        assert!(matches!(
            dispatch(command(&["ECHO", "a", "b"])),
            RespTypes::Error(_)
        ));
    }

    #[test]
    fn unknown_command_names_the_command_it_did_not_know() {
        match dispatch(command(&["SET", "a", "b"])) {
            RespTypes::Error(msg) => assert!(msg.contains("SET"), "got: {msg}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_array_is_an_error_not_a_panic() {
        // Without the `!parts.is_empty()` guard this would index into an empty
        // vec and take the whole server down.
        assert!(matches!(
            dispatch(RespTypes::Array(Some(vec![]))),
            RespTypes::Error(_)
        ));
    }

    #[test]
    fn frames_that_are_not_arrays_are_errors() {
        assert!(matches!(
            dispatch(RespTypes::SimpleString("PING".to_string())),
            RespTypes::Error(_)
        ));
        assert!(matches!(
            dispatch(RespTypes::Array(None)),
            RespTypes::Error(_)
        ));
    }

    #[test]
    fn a_command_name_that_is_not_valid_utf8_is_an_error() {
        let frame = RespTypes::Array(Some(vec![RespTypes::BulkString(Some(vec![0xff, 0xfe]))]));
        assert!(matches!(dispatch(frame), RespTypes::Error(_)));
    }
}
