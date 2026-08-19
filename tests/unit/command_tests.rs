//! Unit tests for `src/dispatch.rs`.
//!
//! Pulled into the crate by the `#[path]` stub at the bottom of that file, so
//! these are still part of the `dispatch` module: they run with `cargo test`,
//! they see `use super::*`, and they can reach private items like `key_of`.
//! Only the file they live in changed.
//!
//! Every test here is `dispatch(frame, db) == frame` — no socket, no runtime,
//! no async. That is the payoff for keeping the command layer free of both.

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

fn ok() -> RespTypes {
    RespTypes::SimpleString("OK".to_string())
}

/// Runs a command against a throwaway store, for the cases where state does
/// not matter.
fn run(parts: &[&str]) -> RespTypes {
    dispatch(command(parts), &mut Db::default())
}

// --- PING / ECHO -----------------------------------------------------------

#[test]
fn ping_with_no_arguments_replies_pong() {
    assert_eq!(run(&["PING"]), RespTypes::SimpleString("PONG".to_string()));
}

#[test]
fn ping_with_one_argument_replies_with_that_argument() {
    assert_eq!(run(&["PING", "hello"]), bulk("hello"));
}

#[test]
fn echo_replies_with_its_argument() {
    assert_eq!(run(&["ECHO", "hello"]), bulk("hello"));
}

#[test]
fn command_names_are_case_insensitive() {
    assert_eq!(run(&["ping"]), RespTypes::SimpleString("PONG".to_string()));
    assert_eq!(run(&["EcHo", "hi"]), bulk("hi"));
}

#[test]
fn echo_without_an_argument_is_an_error() {
    assert!(matches!(run(&["ECHO"]), RespTypes::Error(_)));
}

#[test]
fn echo_with_too_many_arguments_is_an_error() {
    assert!(matches!(run(&["ECHO", "a", "b"]), RespTypes::Error(_)));
}

// --- SET / GET -------------------------------------------------------------

#[test]
fn set_then_get_returns_the_value() {
    let mut db = Db::default();
    assert_eq!(dispatch(command(&["SET", "k", "v"]), &mut db), ok());
    assert_eq!(dispatch(command(&["GET", "k"]), &mut db), bulk("v"));
}

#[test]
fn set_over_an_existing_key_replies_ok_and_replaces_the_value() {
    // Overwriting is normal in Redis, not an error.
    let mut db = Db::default();
    dispatch(command(&["SET", "k", "one"]), &mut db);
    assert_eq!(dispatch(command(&["SET", "k", "two"]), &mut db), ok());
    assert_eq!(dispatch(command(&["GET", "k"]), &mut db), bulk("two"));
}

#[test]
fn get_on_a_missing_key_is_the_null_bulk_string() {
    // `redis-cli` prints this as `(nil)`. A plain string would look like a
    // value that was actually stored.
    assert_eq!(run(&["GET", "nope"]), RespTypes::BulkString(None));
}

#[test]
fn an_empty_value_is_stored_and_returned() {
    let mut db = Db::default();
    assert_eq!(dispatch(command(&["SET", "k", ""]), &mut db), ok());
    assert_eq!(dispatch(command(&["GET", "k"]), &mut db), bulk(""));
}

#[test]
fn values_may_be_arbitrary_bytes() {
    // Not valid UTF-8. A value can be a JPEG, which is why args stay Vec<u8>
    // all the way from the socket to the store.
    let mut db = Db::default();
    let value = vec![0xff, 0x00, 0xfe];
    let set = RespTypes::Array(Some(vec![
        RespTypes::BulkString(Some(b"SET".to_vec())),
        RespTypes::BulkString(Some(b"blob".to_vec())),
        RespTypes::BulkString(Some(value.clone())),
    ]));
    assert_eq!(dispatch(set, &mut db), ok());
    assert_eq!(
        dispatch(command(&["GET", "blob"]), &mut db),
        RespTypes::BulkString(Some(value))
    );
}

#[test]
fn set_and_get_check_their_argument_count() {
    assert!(matches!(run(&["SET", "k"]), RespTypes::Error(_)));
    assert!(matches!(run(&["SET", "k", "v", "extra"]), RespTypes::Error(_)));
    assert!(matches!(run(&["GET"]), RespTypes::Error(_)));
    assert!(matches!(run(&["GET", "a", "b"]), RespTypes::Error(_)));
}

// --- DEL / EXISTS / TYPE ---------------------------------------------------

#[test]
fn del_reports_how_many_keys_it_removed() {
    let mut db = Db::default();
    dispatch(command(&["SET", "a", "1"]), &mut db);
    dispatch(command(&["SET", "b", "2"]), &mut db);
    assert_eq!(
        dispatch(command(&["DEL", "a", "b", "missing"]), &mut db),
        RespTypes::Integer(2)
    );
    assert_eq!(dispatch(command(&["GET", "a"]), &mut db), RespTypes::BulkString(None));
}

#[test]
fn del_on_a_missing_key_is_zero_not_an_error() {
    assert_eq!(run(&["DEL", "nope"]), RespTypes::Integer(0));
}

#[test]
fn exists_counts_the_keys_that_are_there() {
    let mut db = Db::default();
    dispatch(command(&["SET", "a", "1"]), &mut db);
    assert_eq!(
        dispatch(command(&["EXISTS", "a", "missing"]), &mut db),
        RespTypes::Integer(1)
    );
    // Repeats count twice — this is Redis behaviour, not a bug.
    assert_eq!(
        dispatch(command(&["EXISTS", "a", "a"]), &mut db),
        RespTypes::Integer(2)
    );
}

#[test]
fn type_names_the_kind_of_value_and_says_none_when_missing() {
    let mut db = Db::default();
    dispatch(command(&["SET", "k", "v"]), &mut db);
    assert_eq!(
        dispatch(command(&["TYPE", "k"]), &mut db),
        RespTypes::SimpleString("string".to_string())
    );
    assert_eq!(
        dispatch(command(&["TYPE", "missing"]), &mut db),
        RespTypes::SimpleString("none".to_string())
    );
}

// --- malformed input -------------------------------------------------------

#[test]
fn unknown_command_names_the_command_it_did_not_know() {
    match run(&["FLURB", "a"]) {
        RespTypes::Error(msg) => assert!(msg.contains("FLURB"), "got: {msg}"),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn an_empty_array_is_an_error_not_a_panic() {
    // Without the `!parts.is_empty()` guard this would index into an empty vec
    // and take the whole server down.
    assert!(matches!(
        dispatch(RespTypes::Array(Some(vec![])), &mut Db::default()),
        RespTypes::Error(_)
    ));
}

#[test]
fn frames_that_are_not_arrays_are_errors() {
    let mut db = Db::default();
    assert!(matches!(
        dispatch(RespTypes::SimpleString("PING".to_string()), &mut db),
        RespTypes::Error(_)
    ));
    assert!(matches!(
        dispatch(RespTypes::Array(None), &mut db),
        RespTypes::Error(_)
    ));
}

#[test]
fn a_command_name_that_is_not_valid_utf8_is_an_error() {
    let frame = RespTypes::Array(Some(vec![RespTypes::BulkString(Some(vec![0xff, 0xfe]))]));
    assert!(matches!(
        dispatch(frame, &mut Db::default()),
        RespTypes::Error(_)
    ));
}

#[test]
fn an_element_that_is_not_a_bulk_string_is_an_error() {
    let frame = RespTypes::Array(Some(vec![
        RespTypes::BulkString(Some(b"GET".to_vec())),
        RespTypes::Integer(5),
    ]));
    assert!(matches!(
        dispatch(frame, &mut Db::default()),
        RespTypes::Error(_)
    ));
}
