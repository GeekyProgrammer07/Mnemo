//! Ported from `redis/tests/unit/type/hash.tcl`.
//!
//! Covers setting and reading fields, deleting them, the bulk readers
//! HGETALL / HKEYS / HVALS / HMGET, and the HINCRBY pair.
//!
//! Skipped: the listpack/hashtable encoding checks, and the hash-field-expire
//! family (HEXPIRE, HGETEX, ...) -- a Redis 7.4 addition past the roadmap.
//!
//! Use `HashMap<Vec<u8>, Vec<u8>>`: every access is a single field lookup, and
//! hashes promise no field order, which is exactly what a hash map gives.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// HSET / HGET
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 8: hash commands (HSET/HGET)"]
fn hset_and_hget_basic() {
    // HSET counts new fields, not writes: first HSET h f v1 => 1,
    // overwriting with HSET h f v2 => 0 even though the value did change.
    let mut c = connect();
    c.del(&["hsh_h"]);
    assert_eq!(c.cmd(&["HSET", "hsh_h", "field1", "v1"]), int(1));
    assert_eq!(c.cmd(&["HGET", "hsh_h", "field1"]), bulk("v1"));
    assert_eq!(c.cmd(&["HSET", "hsh_h", "field1", "v2"]), int(0));
    assert_eq!(c.cmd(&["HGET", "hsh_h", "field1"]), bulk("v2"));
    c.del(&["hsh_h"]);
}

#[test]
#[ignore = "Session 8: variadic HSET"]
fn hset_with_multiple_field_value_pairs() {
    // Takes any number of field/value pairs and still counts only new fields.
    let mut c = connect();
    c.del(&["hsh_h"]);
    assert_eq!(c.cmd(&["HSET", "hsh_h", "f1", "v1", "f2", "v2", "f3", "v3"]), int(3));
    assert_eq!(c.cmd(&["HLEN", "hsh_h"]), int(3));
    // f1 already exists, f4 does not => 1.
    assert_eq!(c.cmd(&["HSET", "hsh_h", "f1", "x", "f4", "v4"]), int(1));
    // A dangling field with no value is a wrong-arity error.
    assert_error(&c.cmd(&["HSET", "hsh_h", "f5", "v5", "f6"]), "ERR wrong number");
    c.del(&["hsh_h"]);
}

#[test]
#[ignore = "Session 8: hash commands (HGET against missing field/key)"]
fn hget_against_non_existing_field_or_key() {
    // Missing field and missing key both give nil, so HGET alone cannot tell
    // them apart. HEXISTS is the command that can.
    let mut c = connect();
    c.del(&["hsh_h"]);
    c.cmd(&["HSET", "hsh_h", "f1", "v1"]);
    assert!(c.cmd(&["HGET", "hsh_h", "nofield"]).is_nil());
    c.del(&["hsh_missing"]);
    assert!(c.cmd(&["HGET", "hsh_missing", "f1"]).is_nil());
}

#[test]
#[ignore = "Session 8: hash commands (HSETNX)"]
fn hsetnx_only_sets_a_missing_field() {
    // Writes only if the field is absent: second HSETNX h f1 v2 => 0, value stays v1.
    let mut c = connect();
    c.del(&["hsh_nx"]);
    assert_eq!(c.cmd(&["HSETNX", "hsh_nx", "f1", "v1"]), int(1));
    assert_eq!(c.cmd(&["HSETNX", "hsh_nx", "f1", "v2"]), int(0));
    assert_eq!(c.cmd(&["HGET", "hsh_nx", "f1"]), bulk("v1"));
    c.del(&["hsh_nx"]);
}

// ---------------------------------------------------------------------------
// HDEL / HEXISTS / HLEN / HSTRLEN
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 8: hash commands (HDEL)"]
fn hdel_removes_fields_and_counts_them() {
    // Counts only fields that were there: HDEL h f2 f3 nofield => 2.
    let mut c = connect();
    c.del(&["hsh_d"]);
    c.cmd(&["HSET", "hsh_d", "f1", "v1", "f2", "v2", "f3", "v3"]);
    assert_eq!(c.cmd(&["HDEL", "hsh_d", "f1"]), int(1));
    assert_eq!(c.cmd(&["HDEL", "hsh_d", "nofield"]), int(0));
    assert_eq!(c.cmd(&["HDEL", "hsh_d", "f2", "f3", "nofield"]), int(2));
    c.del(&["hsh_d"]);
}

#[test]
#[ignore = "Session 8: deleting the last field deletes the key"]
fn hdel_of_the_last_field_deletes_the_key() {
    // No empty collections, same as lists and sets: delete the last field and
    // EXISTS is 0, TYPE is none. One shared "drop the key if empty" helper.
    let mut c = connect();
    c.del(&["hsh_e"]);
    c.cmd(&["HSET", "hsh_e", "f1", "v1"]);
    assert_eq!(c.cmd(&["HDEL", "hsh_e", "f1"]), int(1));
    assert_eq!(c.cmd(&["EXISTS", "hsh_e"]), int(0));
    assert_eq!(c.cmd(&["TYPE", "hsh_e"]), simple("none"));
}

#[test]
#[ignore = "Session 8: hash commands (HEXISTS)"]
fn hexists() {
    // 1 or 0, never nil -- and a missing key answers 0, not an error.
    let mut c = connect();
    c.del(&["hsh_x"]);
    c.cmd(&["HSET", "hsh_x", "f1", "v1"]);
    assert_eq!(c.cmd(&["HEXISTS", "hsh_x", "f1"]), int(1));
    assert_eq!(c.cmd(&["HEXISTS", "hsh_x", "nofield"]), int(0));
    c.del(&["hsh_missing"]);
    assert_eq!(c.cmd(&["HEXISTS", "hsh_missing", "f1"]), int(0));
}

#[test]
#[ignore = "Session 8: hash commands (HLEN)"]
fn hlen_against_existing_and_missing_keys() {
    // Field count; a missing key is 0, not an error.
    let mut c = connect();
    c.del(&["hsh_l"]);
    assert_eq!(c.cmd(&["HLEN", "hsh_l"]), int(0));
    c.cmd(&["HSET", "hsh_l", "f1", "v1", "f2", "v2"]);
    assert_eq!(c.cmd(&["HLEN", "hsh_l"]), int(2));
    c.del(&["hsh_l"]);
}

#[test]
#[ignore = "bonus: HSTRLEN"]
fn hstrlen_returns_the_field_value_length() {
    // Length of the value in bytes: f1 = "hello" => 5. A missing field is 0.
    let mut c = connect();
    c.del(&["hsh_sl"]);
    c.cmd(&["HSET", "hsh_sl", "f1", "hello"]);
    assert_eq!(c.cmd(&["HSTRLEN", "hsh_sl", "f1"]), int(5));
    assert_eq!(c.cmd(&["HSTRLEN", "hsh_sl", "nofield"]), int(0));
    c.del(&["hsh_sl"]);
}

// ---------------------------------------------------------------------------
// HGETALL / HKEYS / HVALS / HMGET
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 8: hash commands (HGETALL)"]
fn hgetall_returns_a_flat_field_value_array() {
    // RESP2 has no map type, so the reply is one flat array: [f1, v1, f2, v2].
    // Field order is not promised, hence the pairing and sorting below.
    let mut c = connect();
    c.del(&["hsh_ga"]);
    c.cmd(&["HSET", "hsh_ga", "f1", "v1", "f2", "v2"]);
    let flat = c.cmd(&["HGETALL", "hsh_ga"]).strings();
    assert_eq!(flat.len(), 4);
    let mut pairs: Vec<(String, String)> =
        flat.chunks(2).map(|p| (p[0].clone(), p[1].clone())).collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("f1".to_string(), "v1".to_string()),
            ("f2".to_string(), "v2".to_string())
        ]
    );
    c.del(&["hsh_ga"]);
}

#[test]
#[ignore = "Session 8: hash commands (HGETALL against a missing key)"]
fn hgetall_against_non_existing_key() {
    // Empty array, not nil and not an error.
    let mut c = connect();
    c.del(&["hsh_missing"]);
    assert_eq!(c.cmd(&["HGETALL", "hsh_missing"]), arr(vec![]));
}

#[test]
#[ignore = "Session 8: hash commands (HKEYS/HVALS)"]
fn hkeys_and_hvals() {
    // The two halves of HGETALL, in no promised order. A missing key gives an
    // empty array from both.
    let mut c = connect();
    c.del(&["hsh_kv"]);
    c.cmd(&["HSET", "hsh_kv", "f1", "v1", "f2", "v2", "f3", "v3"]);
    assert_eq!(c.cmd(&["HKEYS", "hsh_kv"]).sorted(), vec!["f1", "f2", "f3"]);
    assert_eq!(c.cmd(&["HVALS", "hsh_kv"]).sorted(), vec!["v1", "v2", "v3"]);
    c.del(&["hsh_missing"]);
    assert_eq!(c.cmd(&["HKEYS", "hsh_missing"]), arr(vec![]));
    assert_eq!(c.cmd(&["HVALS", "hsh_missing"]), arr(vec![]));
    c.del(&["hsh_kv"]);
}

#[test]
#[ignore = "Session 8: hash commands (HMGET)"]
fn hmget_returns_nil_for_missing_fields() {
    // One reply slot per field asked for, in order, nil where the field is
    // missing: HMGET h f1 nofield f2 => [v1, nil, v2].
    let mut c = connect();
    c.del(&["hsh_mg"]);
    c.cmd(&["HSET", "hsh_mg", "f1", "v1", "f2", "v2"]);
    assert_eq!(
        c.cmd(&["HMGET", "hsh_mg", "f1", "nofield", "f2"]),
        arr(vec![bulk("v1"), nil(), bulk("v2")])
    );
    // Missing key: still one nil per field, not an empty array.
    c.del(&["hsh_missing"]);
    assert_eq!(
        c.cmd(&["HMGET", "hsh_missing", "f1", "f2"]),
        arr(vec![nil(), nil()])
    );
    c.del(&["hsh_mg"]);
}

// ---------------------------------------------------------------------------
// HINCRBY / HINCRBYFLOAT
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: HINCRBY"]
fn hincrby_against_missing_and_existing_fields() {
    // A missing field starts at 0, so HINCRBY h f 5 on a missing key => 5.
    // The value is stored as the string "-5", not as a number.
    let mut c = connect();
    c.del(&["hsh_ib"]);
    assert_eq!(c.cmd(&["HINCRBY", "hsh_ib", "f", "5"]), int(5));
    assert_eq!(c.cmd(&["HINCRBY", "hsh_ib", "f", "5"]), int(10));
    assert_eq!(c.cmd(&["HINCRBY", "hsh_ib", "f", "-15"]), int(-5));
    assert_eq!(c.cmd(&["HGET", "hsh_ib", "f"]), bulk("-5"));
    c.del(&["hsh_ib"]);
}

#[test]
#[ignore = "bonus: HINCRBY error cases"]
fn hincrby_against_non_numeric_field_and_overflow() {
    // Same rules as INCR: a non-numeric value errors, and i64::MAX + 1 errors
    // instead of wrapping. Share one helper with INCR so they cannot drift.
    let mut c = connect();
    c.del(&["hsh_ib"]);
    c.cmd(&["HSET", "hsh_ib", "f", "notanumber"]);
    assert_error(&c.cmd(&["HINCRBY", "hsh_ib", "f", "1"]), "ERR hash value is not an integer");

    c.cmd(&["HSET", "hsh_ib", "f", "9223372036854775807"]);
    assert_error(&c.cmd(&["HINCRBY", "hsh_ib", "f", "1"]), "ERR increment or decrement would overflow");
    c.del(&["hsh_ib"]);
}

#[test]
#[ignore = "bonus: HINCRBYFLOAT"]
fn hincrbyfloat() {
    // Float version, with its own error text: "hash value is not a float".
    let mut c = connect();
    c.del(&["hsh_if"]);
    assert_float(&c.cmd(&["HINCRBYFLOAT", "hsh_if", "f", "10.5"]), 10.5);
    assert_float(&c.cmd(&["HINCRBYFLOAT", "hsh_if", "f", "0.1"]), 10.6);
    c.cmd(&["HSET", "hsh_if", "g", "notanumber"]);
    assert_error(
        &c.cmd(&["HINCRBYFLOAT", "hsh_if", "g", "1.0"]),
        "ERR hash value is not a float",
    );
    c.del(&["hsh_if"]);
}

// ---------------------------------------------------------------------------
// Type errors and volume
//
// Rule: a hash command on a key holding another type replies WRONGTYPE and
// changes nothing. SET k v then HSET k f v must fail, not overwrite.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 8: hash commands type checking"]
fn hash_commands_against_a_string_key() {
    let mut c = connect();
    c.del(&["hsh_str"]);
    c.cmd(&["SET", "hsh_str", "v"]);
    assert_wrongtype(&c.cmd(&["HSET", "hsh_str", "f", "v"]));
    assert_wrongtype(&c.cmd(&["HGET", "hsh_str", "f"]));
    assert_wrongtype(&c.cmd(&["HDEL", "hsh_str", "f"]));
    assert_wrongtype(&c.cmd(&["HLEN", "hsh_str"]));
    assert_wrongtype(&c.cmd(&["HGETALL", "hsh_str"]));
    assert_wrongtype(&c.cmd(&["HKEYS", "hsh_str"]));
    assert_wrongtype(&c.cmd(&["HVALS", "hsh_str"]));
    assert_wrongtype(&c.cmd(&["HEXISTS", "hsh_str", "f"]));
    assert_wrongtype(&c.cmd(&["HMGET", "hsh_str", "f"]));
    assert_wrongtype(&c.cmd(&["HINCRBY", "hsh_str", "f", "1"]));
    c.del(&["hsh_str"]);
}

#[test]
#[ignore = "Session 8: hash commands with binary field names"]
fn hash_fields_are_binary_safe() {
    // Field names are raw bytes: b"\x00\xff\r\nfield" works as a field.
    // A `HashMap<String, _>` cannot hold that -- keys must be `Vec<u8>`.
    let mut c = connect();
    c.del(&["hsh_bin"]);
    let field: &[u8] = b"\x00\xff\r\nfield";
    assert_eq!(c.cmd::<&[u8]>(&[b"HSET", b"hsh_bin", field, b"v"]), int(1));
    assert_eq!(c.cmd::<&[u8]>(&[b"HGET", b"hsh_bin", field]), bulk("v"));
    assert_eq!(c.cmd::<&[u8]>(&[b"HEXISTS", b"hsh_bin", field]), int(1));
    c.del(&["hsh_bin"]);
}

#[test]
#[ignore = "Session 8: hash commands under load"]
fn hash_with_many_fields() {
    // 1000 fields: HGETALL is 2000 entries (field + value each), HKEYS is 1000.
    let mut c = connect();
    c.del(&["hsh_big"]);
    for i in 0..1000 {
        c.cmd(&["HSET", "hsh_big", &format!("f{i}"), &format!("v{i}")]);
    }
    assert_eq!(c.cmd(&["HLEN", "hsh_big"]), int(1000));
    assert_eq!(c.cmd(&["HGET", "hsh_big", "f500"]), bulk("v500"));
    assert_eq!(c.cmd(&["HGETALL", "hsh_big"]).array().len(), 2000);
    assert_eq!(c.cmd(&["HKEYS", "hsh_big"]).array().len(), 1000);
    c.del(&["hsh_big"]);
}
