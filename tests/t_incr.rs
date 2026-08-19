//! Ported from `redis/tests/unit/type/incr.tcl`.
//!
//! INCR/DECR/INCRBYFLOAT. Numbers here are stored as text, so every call parses
//! the string again — most of these tests are about which strings count.
//!
//! Skipped from the original: `assert_refcount` and `DEBUG OBJECT`, which poke at
//! C memory layout you don't have.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// INCR / DECR
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 5: INCR"]
fn incr_against_non_existing_key() {
    // Missing key counts as 0, so the first INCR gives 1 and creates the key.
    // INCR replies with the integer 1, but GET returns the bulk string "1".
    let mut c = connect();
    c.del(&["incr_novar"]);
    assert_eq!(c.cmd(&["INCR", "incr_novar"]), int(1));
    assert_eq!(c.cmd(&["GET", "incr_novar"]), bulk("1"));
    assert_eq!(c.cmd(&["INCR", "incr_novar"]), int(2));
    assert_eq!(c.cmd(&["GET", "incr_novar"]), bulk("2"));
}

#[test]
#[ignore = "Session 5: DECR"]
fn decr_against_key_created_by_incr_and_against_missing_key() {
    // DECR on a missing key gives -1, and INCR on that key brings it back to 0.
    let mut c = connect();
    c.del(&["incr_novar"]);
    c.cmd(&["INCR", "incr_novar"]);
    c.cmd(&["INCR", "incr_novar"]);
    assert_eq!(c.cmd(&["DECR", "incr_novar"]), int(1));

    c.del(&["incr_missing"]);
    assert_eq!(c.cmd(&["DECR", "incr_missing"]), int(-1));
    assert_eq!(c.cmd(&["INCR", "incr_missing"]), int(0));
}

#[test]
#[ignore = "Session 5: INCR against a key set with SET"]
fn incr_against_key_originally_set_with_set() {
    // A key written by SET is the same thing as one written by INCR: SET "100"
    // then INCR gives 101. There is no separate "counter" type.
    let mut c = connect();
    c.cmd(&["SET", "incr_novar", "100"]);
    assert_eq!(c.cmd(&["INCR", "incr_novar"]), int(101));
}

#[test]
#[ignore = "Session 5: INCR over 32-bit values"]
fn incr_over_32bit_value() {
    // 17179869184 needs more than 32 bits. Use i64 everywhere — an i32 anywhere
    // in the path silently truncates instead of erroring.
    let mut c = connect();
    c.cmd(&["SET", "incr_novar", "17179869184"]);
    assert_eq!(c.cmd(&["INCR", "incr_novar"]), int(17179869185));

    c.cmd(&["SET", "incr_novar", "17179869184"]);
    assert_eq!(c.cmd(&["INCRBY", "incr_novar", "17179869184"]), int(34359738368));
}

#[test]
#[ignore = "Session 5: DECRBY over 32-bit values"]
fn decrby_over_32bit_value_with_negative_result() {
    let mut c = connect();
    c.cmd(&["SET", "incr_novar", "17179869184"]);
    assert_eq!(c.cmd(&["DECRBY", "incr_novar", "17179869185"]), int(-1));

    c.del(&["incr_missing"]);
    assert_eq!(c.cmd(&["DECRBY", "incr_missing", "1"]), int(-1));
}

// ---------------------------------------------------------------------------
// Parsing rules
//
// Redis re-reads the stored text on every INCR, and its idea of "an integer" is
// stricter than most parsers. The value must round-trip exactly: if " 11" could
// be incremented to "12", you'd have silently rewritten the user's data.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 5: INCR rejects values with surrounding whitespace"]
fn incr_fails_against_key_with_spaces() {
    // "    11" is not an integer. Don't trim before parsing.
    let mut c = connect();
    for value in ["    11", "11    ", "    11    "] {
        c.cmd(&["SET", "incr_sp", value]);
        assert_error(&c.cmd(&["INCR", "incr_sp"]), "ERR value is not an integer");
    }
    c.del(&["incr_sp"]);
}

#[test]
#[ignore = "Session 5: INCR rejects non-numeric values"]
fn incr_fails_against_non_numeric_values() {
    // "3.0", "+11" and "0x10" all look numeric but are rejected. Rust's
    // str::parse::<i64>() gets every case here right already.
    let mut c = connect();
    for value in ["foobar", "", "3.0", "+11", "0x10", "11abc"] {
        c.cmd(&["SET", "incr_bad", value]);
        assert_error(
            &c.cmd(&["INCR", "incr_bad"]),
            "ERR value is not an integer or out of range",
        );
    }
    c.del(&["incr_bad"]);
}

#[test]
#[ignore = "Session 5: INCRBY rejects a non-numeric increment"]
fn incrby_rejects_non_numeric_increment() {
    // Check the argument before creating the key: a failed INCRBY on a missing
    // key must leave no key behind.
    let mut c = connect();
    c.del(&["incr_arg"]);
    assert_error(
        &c.cmd(&["INCRBY", "incr_arg", "notanumber"]),
        "ERR value is not an integer or out of range",
    );
    assert_eq!(c.cmd(&["EXISTS", "incr_arg"]), int(0));
}

#[test]
#[ignore = "Session 5: INCR type checking"]
fn incr_fails_against_a_key_holding_a_list() {
    // Type check before parsing: a list gives WRONGTYPE, not "not an integer".
    let mut c = connect();
    c.del(&["incr_list"]);
    c.cmd(&["RPUSH", "incr_list", "1"]);
    assert_wrongtype(&c.cmd(&["INCR", "incr_list"]));
    assert_wrongtype(&c.cmd(&["INCRBY", "incr_list", "1"]));
    assert_wrongtype(&c.cmd(&["DECR", "incr_list"]));
    c.del(&["incr_list"]);
}

// ---------------------------------------------------------------------------
// Overflow
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 5: INCR overflow must error, not wrap"]
fn incr_overflow_is_an_error() {
    // i64::MAX + 1 must error, not wrap or panic: use checked_add and turn None
    // into "ERR increment or decrement would overflow".
    let mut c = connect();
    c.cmd(&["SET", "incr_max", "9223372036854775807"]);
    assert_error(
        &c.cmd(&["INCR", "incr_max"]),
        "ERR increment or decrement would overflow",
    );
    // A failed INCR leaves the stored value alone.
    assert_eq!(c.cmd(&["GET", "incr_max"]), bulk("9223372036854775807"));

    c.cmd(&["SET", "incr_min", "-9223372036854775808"]);
    assert_error(
        &c.cmd(&["DECR", "incr_min"]),
        "ERR increment or decrement would overflow",
    );
    c.del(&["incr_max", "incr_min"]);
}

#[test]
#[ignore = "Session 5: DECRBY negation overflow"]
fn decrby_negation_overflow() {
    // -(i64::MIN) does not fit in an i64, so writing DECRBY as `INCRBY -n`
    // overflows on the negation itself, before the stored value is even read.
    let mut c = connect();
    c.cmd(&["SET", "incr_x", "0"]);
    assert_error(&c.cmd(&["DECRBY", "incr_x", "-9223372036854775808"]), "ERR");
    c.del(&["incr_x"]);
}

#[test]
#[ignore = "Session 5: INCR rejects out-of-range stored values"]
fn incr_rejects_stored_value_out_of_i64_range() {
    // "17179869184000000000000" is a real number but too big for i64, so it fails
    // at parse time — same error as "foobar", not an overflow error.
    let mut c = connect();
    c.cmd(&["SET", "incr_huge", "17179869184000000000000"]);
    assert_error(
        &c.cmd(&["INCR", "incr_huge"]),
        "ERR value is not an integer or out of range",
    );
    c.del(&["incr_huge"]);
}

// ---------------------------------------------------------------------------
// INCRBYFLOAT
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: INCRBYFLOAT"]
fn incrbyfloat_against_non_existing_key() {
    // The reply is a bulk string ("1.25"), not an integer — RESP2 has no float
    // type, so floats travel as text.
    let mut c = connect();
    c.del(&["incr_f"]);
    assert_float(&c.cmd(&["INCRBYFLOAT", "incr_f", "1"]), 1.0);
    assert_float(&c.cmd(&["GET", "incr_f"]), 1.0);
    assert_float(&c.cmd(&["INCRBYFLOAT", "incr_f", "0.25"]), 1.25);
    assert_float(&c.cmd(&["GET", "incr_f"]), 1.25);
    c.del(&["incr_f"]);
}

#[test]
#[ignore = "bonus: INCRBYFLOAT"]
fn incrbyfloat_against_key_originally_set_with_set() {
    let mut c = connect();
    c.cmd(&["SET", "incr_f", "1.5"]);
    assert_float(&c.cmd(&["INCRBYFLOAT", "incr_f", "1.5"]), 3.0);

    c.cmd(&["SET", "incr_f", "17179869184"]);
    assert_float(&c.cmd(&["INCRBYFLOAT", "incr_f", "1.5"]), 17179869185.5);
    c.del(&["incr_f"]);
}

#[test]
#[ignore = "bonus: INCRBYFLOAT trims trailing zeros"]
fn incrbyfloat_formats_the_result_without_trailing_zeros() {
    // 3.0 comes back as "3", not "3.0" — Redis formats with %.17Lg and strips
    // trailing zeros. That exact text is what gets stored and re-parsed next time.
    let mut c = connect();
    c.del(&["incr_f"]);
    assert_eq!(c.cmd(&["INCRBYFLOAT", "incr_f", "3.0"]), bulk("3"));
    assert_eq!(c.cmd(&["GET", "incr_f"]), bulk("3"));
    assert_eq!(c.cmd(&["INCRBYFLOAT", "incr_f", "1.500"]), bulk("4.5"));
    c.del(&["incr_f"]);
}

#[test]
#[ignore = "bonus: INCRBYFLOAT error cases"]
fn incrbyfloat_fails_against_non_float_values() {
    let mut c = connect();
    c.cmd(&["SET", "incr_f", "foo"]);
    assert_error(&c.cmd(&["INCRBYFLOAT", "incr_f", "1.0"]), "ERR value is not a valid float");

    c.cmd(&["SET", "incr_f", "1.0"]);
    assert_error(&c.cmd(&["INCRBYFLOAT", "incr_f", "foo"]), "ERR value is not a valid float");

    // "nan" and "inf" are rejected, never stored: once a key holds NaN, every
    // later INCRBYFLOAT on it fails and the value can't be fixed.
    c.cmd(&["SET", "incr_f", "1.0"]);
    assert_error(&c.cmd(&["INCRBYFLOAT", "incr_f", "nan"]), "ERR value is not a valid float");
    assert_error(&c.cmd(&["INCRBYFLOAT", "incr_f", "inf"]), "ERR");
    c.del(&["incr_f"]);
}

#[test]
#[ignore = "bonus: INCRBYFLOAT type checking"]
fn incrbyfloat_fails_against_a_key_holding_a_list() {
    let mut c = connect();
    c.del(&["incr_flist"]);
    c.cmd(&["RPUSH", "incr_flist", "1"]);
    assert_wrongtype(&c.cmd(&["INCRBYFLOAT", "incr_flist", "1.0"]));
    c.del(&["incr_flist"]);
}
