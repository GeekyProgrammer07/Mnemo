//! Unit tests for `src/resp/parser.rs`.
//!
//! Pulled into the crate by the `#[path]` stub at the bottom of that file, so
//! these are still part of the `resp::parser` module: they run with
//! `cargo test`, they see `use super::*`, and they can reach private items --
//! `read_line` and the `parse_*` helpers are not `pub`, and these tests call
//! them directly. Only the file they live in changed.

use super::*;

// --- read_line ---------------------------------------------------------

#[test]
fn read_line_returns_the_line_and_moves_past_crlf() {
    let mut pos = 0;
    assert_eq!(read_line(b"+OK\r\n", &mut pos), Ok(&b"+OK"[..]));
    assert_eq!(pos, 5);
}

#[test]
fn read_line_handles_an_empty_line() {
    let mut pos = 0;
    assert_eq!(read_line(b"\r\n", &mut pos), Ok(&b""[..]));
    assert_eq!(pos, 2);
}

#[test]
fn read_line_on_empty_buffer_does_not_panic() {
    // A client can connect and send nothing. Indexing blindly would crash
    // the whole server here.
    let mut pos = 0;
    assert_eq!(read_line(b"", &mut pos), Err(ParseError::Incomplete));
}

#[test]
fn read_line_without_crlf_is_incomplete() {
    let mut pos = 0;
    assert_eq!(read_line(b"+OK", &mut pos), Err(ParseError::Incomplete));
    // Nothing was consumed, so we can retry after more bytes arrive.
    assert_eq!(pos, 0);
}

#[test]
fn read_line_with_cr_but_no_lf_yet_is_incomplete() {
    // TCP split the message right between \r and \n.
    let mut pos = 0;
    assert_eq!(read_line(b"+OK\r", &mut pos), Err(ParseError::Incomplete));
}

#[test]
fn read_line_rejects_a_lone_cr() {
    // A \r not followed by \n is never valid RESP.
    let mut pos = 0;
    assert!(matches!(
        read_line(b"+O\rK\r\n", &mut pos),
        Err(ParseError::Protocol(_))
    ));
}

// --- the three one-line types -----------------------------------------

#[test]
fn parses_a_simple_string() {
    let mut pos = 0;
    assert_eq!(
        parse(b"+OK\r\n", &mut pos),
        Ok(RespTypes::SimpleString("OK".to_string()))
    );
    assert_eq!(pos, 5);
}

#[test]
fn parses_an_empty_simple_string() {
    let mut pos = 0;
    assert_eq!(
        parse(b"+\r\n", &mut pos),
        Ok(RespTypes::SimpleString(String::new()))
    );
}

#[test]
fn parses_an_error() {
    let mut pos = 0;
    assert_eq!(
        parse(b"-ERR unknown command\r\n", &mut pos),
        Ok(RespTypes::Error("ERR unknown command".to_string()))
    );
}

#[test]
fn parses_an_integer() {
    let mut pos = 0;
    assert_eq!(parse(b":42\r\n", &mut pos), Ok(RespTypes::Integer(42)));
    assert_eq!(pos, 5);
}

#[test]
fn parses_a_negative_integer() {
    let mut pos = 0;
    assert_eq!(parse(b":-1\r\n", &mut pos), Ok(RespTypes::Integer(-1)));
}

#[test]
fn integer_that_is_not_a_number_is_a_protocol_error() {
    let mut pos = 0;
    assert!(matches!(
        parse(b":abc\r\n", &mut pos),
        Err(ParseError::Protocol(_))
    ));
}

// --- bulk strings ------------------------------------------------------

#[test]
fn parses_a_bulk_string() {
    let mut pos = 0;
    assert_eq!(
        parse(b"$5\r\nhello\r\n", &mut pos),
        Ok(RespTypes::BulkString(Some(b"hello".to_vec())))
    );
    assert_eq!(pos, 11);
}

#[test]
fn parses_an_empty_bulk_string() {
    let mut pos = 0;
    assert_eq!(
        parse(b"$0\r\n\r\n", &mut pos),
        Ok(RespTypes::BulkString(Some(vec![])))
    );
    assert_eq!(pos, 6);
}

#[test]
fn parses_a_null_bulk_string() {
    // What GET returns when the key is missing.
    let mut pos = 0;
    assert_eq!(parse(b"$-1\r\n", &mut pos), Ok(RespTypes::BulkString(None)));
    assert_eq!(pos, 5);
}

#[test]
fn bulk_string_payload_may_contain_crlf() {
    // The reason the payload is taken by length instead of by scanning:
    // these 5 bytes include a \r\n that is data, not a terminator.
    let mut pos = 0;
    assert_eq!(
        parse(b"$5\r\na\r\nbc\r\n", &mut pos),
        Ok(RespTypes::BulkString(Some(b"a\r\nbc".to_vec())))
    );
}

#[test]
fn bulk_string_payload_may_be_binary() {
    let mut pos = 0;
    assert_eq!(
        parse(b"$3\r\n\xff\x00\xfe\r\n", &mut pos),
        Ok(RespTypes::BulkString(Some(vec![0xff, 0x00, 0xfe])))
    );
}

#[test]
fn half_delivered_bulk_string_is_incomplete_not_a_panic() {
    // Slicing without a bounds check here would crash the whole server.
    let mut pos = 0;
    assert_eq!(parse(b"$5\r\nhel", &mut pos), Err(ParseError::Incomplete));
    // Cursor is back at the start so the frame can be retried.
    assert_eq!(pos, 0);
}

#[test]
fn bulk_string_missing_its_trailing_crlf_is_a_protocol_error() {
    let mut pos = 0;
    assert!(matches!(
        parse(b"$5\r\nhelloXX", &mut pos),
        Err(ParseError::Protocol(_))
    ));
}

#[test]
fn bulk_string_with_only_one_bad_terminator_byte_is_rejected() {
    // Catches `&&` where `||` is meant: here only the first byte is wrong,
    // so an `&&` check would wave this through.
    let mut pos = 0;
    assert!(matches!(
        parse(b"$5\r\nhelloX\n", &mut pos),
        Err(ParseError::Protocol(_))
    ));

    // And the mirror case: only the second byte is wrong.
    let mut pos = 0;
    assert!(matches!(
        parse(b"$5\r\nhello\rX", &mut pos),
        Err(ParseError::Protocol(_))
    ));
}

#[test]
fn negative_bulk_length_other_than_minus_one_is_a_protocol_error() {
    let mut pos = 0;
    assert!(matches!(
        parse(b"$-2\r\nab\r\n", &mut pos),
        Err(ParseError::Protocol(_))
    ));
}

#[test]
fn reads_two_bulk_strings_from_one_buffer() {
    let buf = b"$4\r\nECHO\r\n$5\r\nhello\r\n";
    let mut pos = 0;

    assert_eq!(
        parse(buf, &mut pos),
        Ok(RespTypes::BulkString(Some(b"ECHO".to_vec())))
    );
    assert_eq!(
        parse(buf, &mut pos),
        Ok(RespTypes::BulkString(Some(b"hello".to_vec())))
    );
    assert_eq!(pos, buf.len());
}

// --- arrays ------------------------------------------------------------

#[test]
fn parses_a_command_array() {
    // What a real client actually sends for: ECHO hello
    let buf = b"*2\r\n$4\r\nECHO\r\n$5\r\nhello\r\n";
    let mut pos = 0;
    assert_eq!(
        parse(buf, &mut pos),
        Ok(RespTypes::Array(Some(vec![
            RespTypes::BulkString(Some(b"ECHO".to_vec())),
            RespTypes::BulkString(Some(b"hello".to_vec())),
        ])))
    );
    assert_eq!(pos, buf.len());
}

#[test]
fn parses_an_empty_array() {
    let mut pos = 0;
    assert_eq!(
        parse(b"*0\r\n", &mut pos),
        Ok(RespTypes::Array(Some(vec![])))
    );
    assert_eq!(pos, 4);
}

#[test]
fn parses_a_null_array() {
    let mut pos = 0;
    assert_eq!(parse(b"*-1\r\n", &mut pos), Ok(RespTypes::Array(None)));
    assert_eq!(pos, 5);
}

#[test]
fn parses_a_nested_array() {
    // Nesting needs no extra code: reading an element is just `parse`.
    let mut pos = 0;
    assert_eq!(
        parse(b"*1\r\n*2\r\n:5\r\n+OK\r\n", &mut pos),
        Ok(RespTypes::Array(Some(vec![RespTypes::Array(Some(vec![
            RespTypes::Integer(5),
            RespTypes::SimpleString("OK".to_string()),
        ]))])))
    );
}

#[test]
fn parses_an_array_of_mixed_types() {
    let mut pos = 0;
    assert_eq!(
        parse(b"*3\r\n:1\r\n$2\r\nhi\r\n-ERR x\r\n", &mut pos),
        Ok(RespTypes::Array(Some(vec![
            RespTypes::Integer(1),
            RespTypes::BulkString(Some(b"hi".to_vec())),
            RespTypes::Error("ERR x".to_string()),
        ])))
    );
}

#[test]
fn half_delivered_array_rewinds_the_cursor() {
    // The count says 2 but only one element arrived. Without the rewind,
    // pos would be left after `:1\r\n` and the retry would resume
    // mid-array, misreading everything from there on.
    let mut pos = 0;
    assert_eq!(
        parse(b"*2\r\n:1\r\n", &mut pos),
        Err(ParseError::Incomplete)
    );
    assert_eq!(pos, 0);
}

#[test]
fn array_element_that_is_garbage_is_a_protocol_error() {
    let mut pos = 0;
    assert!(matches!(
        parse(b"*1\r\n?bad\r\n", &mut pos),
        Err(ParseError::Protocol(_))
    ));
}

#[test]
fn absurd_array_count_does_not_allocate() {
    // 12 bytes claiming a billion elements. Reserving for that count up
    // front would try for gigabytes of memory.
    let mut pos = 0;
    assert_eq!(
        parse(b"*999999999\r\n", &mut pos),
        Err(ParseError::Incomplete)
    );
    assert_eq!(pos, 0);
}

// --- parse dispatch ----------------------------------------------------

#[test]
fn unknown_marker_is_a_protocol_error() {
    let mut pos = 0;
    assert!(matches!(
        parse(b"?what\r\n", &mut pos),
        Err(ParseError::Protocol(_))
    ));
}

#[test]
fn empty_buffer_is_incomplete() {
    let mut pos = 0;
    assert_eq!(parse(b"", &mut pos), Err(ParseError::Incomplete));
}

#[test]
fn cursor_at_end_of_a_full_buffer_is_incomplete() {
    // The bug `buf.is_empty()` would have missed: buffer is not empty,
    // but there is nothing left to read, so `buf[*pos]` would panic.
    let mut pos = 5;
    assert_eq!(parse(b"+OK\r\n", &mut pos), Err(ParseError::Incomplete));
}

#[test]
fn reads_two_frames_from_one_buffer() {
    // This is what proves `pos` is being carried correctly. If parse left
    // the cursor on the \r, the second call would loop forever.
    let buf = b"+OK\r\n:42\r\n";
    let mut pos = 0;

    assert_eq!(
        parse(buf, &mut pos),
        Ok(RespTypes::SimpleString("OK".to_string()))
    );
    assert_eq!(parse(buf, &mut pos), Ok(RespTypes::Integer(42)));
    assert_eq!(pos, buf.len());
}
