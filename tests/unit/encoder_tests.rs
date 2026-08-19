//! Unit tests for `src/resp/encoder.rs`.
//!
//! Pulled into the crate by the `#[path]` stub at the bottom of that file, so
//! these are still part of the `resp::encoder` module: they run with
//! `cargo test`, they see `use super::*`, and they can reach private items.
//! Only the file they live in changed.

use super::*;
use crate::resp::parser::parse;

/// Encode a value, parse the bytes back, and check nothing changed.
///
/// Catches a wrong length, a missing `\r\n` or a wrong marker in one go,
/// and also proves the two directions agree with each other.
fn round_trip(value: RespTypes) {
    let bytes = encode(&value);
    let mut pos = 0;
    assert_eq!(parse(&bytes, &mut pos), Ok(value));
    // The whole thing was consumed: no bytes left over, none missing.
    assert_eq!(pos, bytes.len());
}

// --- exact bytes on the wire -------------------------------------------
// These say *which* direction broke when a round trip fails.

#[test]
fn encodes_a_simple_string() {
    assert_eq!(
        encode(&RespTypes::SimpleString("OK".to_string())),
        b"+OK\r\n"
    );
}

#[test]
fn encodes_an_error() {
    assert_eq!(
        encode(&RespTypes::Error("ERR unknown command".to_string())),
        b"-ERR unknown command\r\n"
    );
}

#[test]
fn encodes_an_integer() {
    assert_eq!(encode(&RespTypes::Integer(42)), b":42\r\n");
    assert_eq!(encode(&RespTypes::Integer(-1)), b":-1\r\n");
    assert_eq!(encode(&RespTypes::Integer(0)), b":0\r\n");
}

#[test]
fn encodes_a_bulk_string() {
    assert_eq!(
        encode(&RespTypes::BulkString(Some(b"hello".to_vec()))),
        b"$5\r\nhello\r\n"
    );
}

#[test]
fn encodes_an_empty_bulk_string() {
    assert_eq!(encode(&RespTypes::BulkString(Some(vec![]))), b"$0\r\n\r\n");
}

#[test]
fn encodes_a_null_bulk_string() {
    assert_eq!(encode(&RespTypes::BulkString(None)), b"$-1\r\n");
}

#[test]
fn encodes_an_array() {
    assert_eq!(
        encode(&RespTypes::Array(Some(vec![
            RespTypes::BulkString(Some(b"ECHO".to_vec())),
            RespTypes::BulkString(Some(b"hello".to_vec())),
        ]))),
        b"*2\r\n$4\r\nECHO\r\n$5\r\nhello\r\n"
    );
}

#[test]
fn encodes_an_empty_array() {
    assert_eq!(encode(&RespTypes::Array(Some(vec![]))), b"*0\r\n");
}

#[test]
fn encodes_a_null_array() {
    assert_eq!(encode(&RespTypes::Array(None)), b"*-1\r\n");
}

#[test]
fn encodes_a_nested_array() {
    assert_eq!(
        encode(&RespTypes::Array(Some(vec![RespTypes::Array(Some(vec![
            RespTypes::Integer(5),
        ]))]))),
        b"*1\r\n*1\r\n:5\r\n"
    );
}

#[test]
fn bulk_string_length_counts_bytes_not_characters() {
    // "héllo" is 5 characters but 6 bytes: é takes two. Using .chars()
    // or a character count here would write $5 and desync the stream.
    let value = RespTypes::BulkString(Some("héllo".as_bytes().to_vec()));
    assert_eq!(encode(&value), b"$6\r\nh\xc3\xa9llo\r\n");
}

// --- round trips -------------------------------------------------------

#[test]
fn round_trips_a_simple_string() {
    round_trip(RespTypes::SimpleString("PONG".to_string()));
    round_trip(RespTypes::SimpleString(String::new()));
}

#[test]
fn round_trips_an_error() {
    round_trip(RespTypes::Error("WRONGTYPE bad key".to_string()));
}

#[test]
fn round_trips_integers() {
    round_trip(RespTypes::Integer(0));
    round_trip(RespTypes::Integer(-1));
    round_trip(RespTypes::Integer(i64::MAX));
    round_trip(RespTypes::Integer(i64::MIN));
}

#[test]
fn round_trips_bulk_strings() {
    round_trip(RespTypes::BulkString(Some(b"hello".to_vec())));
    round_trip(RespTypes::BulkString(Some(vec![])));
    round_trip(RespTypes::BulkString(None));
}

#[test]
fn round_trips_a_bulk_string_containing_crlf() {
    // The payload holds a \r\n of its own. Survives only because the
    // length header, not a scan, decides where it ends.
    round_trip(RespTypes::BulkString(Some(b"a\r\nb".to_vec())));
}

#[test]
fn round_trips_a_binary_bulk_string() {
    // Not valid UTF-8. This is why the payload never goes through String.
    round_trip(RespTypes::BulkString(Some(vec![0xff, 0x00, 0xfe, 0x0d])));
}

#[test]
fn round_trips_arrays() {
    round_trip(RespTypes::Array(None));
    round_trip(RespTypes::Array(Some(vec![])));
    round_trip(RespTypes::Array(Some(vec![
        RespTypes::Integer(1),
        RespTypes::SimpleString("OK".to_string()),
        RespTypes::BulkString(Some(b"hi".to_vec())),
        RespTypes::BulkString(None),
    ])));
}

#[test]
fn round_trips_a_nested_array() {
    round_trip(RespTypes::Array(Some(vec![
        RespTypes::Array(Some(vec![RespTypes::Integer(1)])),
        RespTypes::Array(None),
        RespTypes::Array(Some(vec![RespTypes::Array(Some(vec![
            RespTypes::SimpleString("deep".to_string()),
        ]))])),
    ])));
}

#[test]
fn two_encoded_values_parse_back_one_after_the_other() {
    // What pipelining looks like: replies concatenated in one write.
    let mut wire = encode(&RespTypes::SimpleString("OK".to_string()));
    wire.extend_from_slice(&encode(&RespTypes::Integer(7)));

    let mut pos = 0;
    assert_eq!(
        parse(&wire, &mut pos),
        Ok(RespTypes::SimpleString("OK".to_string()))
    );
    assert_eq!(parse(&wire, &mut pos), Ok(RespTypes::Integer(7)));
    assert_eq!(pos, wire.len());
}
