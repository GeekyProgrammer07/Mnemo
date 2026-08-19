//! Ported from `redis/tests/unit/type/string.tcl`.
//!
//! SET/GET and the rest of the string commands: the SET options, MGET/MSET,
//! APPEND/STRLEN, and GETRANGE/SETRANGE.
//!
//! Skipped from the original: `DELEX`, `DIGEST`, `IFEQ`/`IFNE`/`IFDEQ`/`IFDNE`,
//! `MSETEX`, `LCS`, `SETBIT`/`GETBIT`, `MEMORY USAGE` — Redis 8 additions, a
//! separate feature area, or an allocator you don't have.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// SET / GET
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 4: in-memory store (SET/GET)"]
fn set_and_get_an_item() {
    let mut c = connect();
    assert_eq!(c.cmd(&["SET", "str_x", "foobar"]), ok());
    assert_eq!(c.cmd(&["GET", "str_x"]), bulk("foobar"));
}

#[test]
#[ignore = "Session 4: in-memory store (SET/GET)"]
fn set_and_get_an_empty_item() {
    let mut c = connect();
    assert_eq!(c.cmd(&["SET", "str_x", ""]), ok());
    assert_eq!(c.cmd(&["GET", "str_x"]), bulk(""));
}

#[test]
#[ignore = "Session 4: in-memory store (GET)"]
fn get_against_non_existing_key() {
    let mut c = connect();
    c.del(&["str_missing"]);
    assert!(c.cmd(&["GET", "str_missing"]).is_nil());
}

#[test]
#[ignore = "Session 4: in-memory store (GET type checking)"]
fn get_against_wrong_type() {
    // Every string command that reads a key must type-check first. The error
    // starts with WRONGTYPE, not ERR — clients match on that exact prefix.
    let mut c = connect();
    c.del(&["str_wt"]);
    c.cmd(&["RPUSH", "str_wt", "a"]);
    assert_wrongtype(&c.cmd(&["GET", "str_wt"]));
    assert_wrongtype(&c.cmd(&["APPEND", "str_wt", "x"]));
    assert_wrongtype(&c.cmd(&["STRLEN", "str_wt"]));
    assert_wrongtype(&c.cmd(&["GETRANGE", "str_wt", "0", "-1"]));
    assert_wrongtype(&c.cmd(&["SETRANGE", "str_wt", "0", "x"]));
    c.del(&["str_wt"]);
}

#[test]
#[ignore = "Session 4: in-memory store (SET overwrites any type)"]
fn set_replaces_a_value_of_any_type() {
    // SET replaces whatever was there, including a list. No WRONGTYPE — unlike GET.
    let mut c = connect();
    c.del(&["str_any"]);
    c.cmd(&["RPUSH", "str_any", "a"]);
    assert_eq!(c.cmd(&["SET", "str_any", "v"]), ok());
    assert_eq!(c.cmd(&["TYPE", "str_any"]), simple("string"));
}

#[test]
#[ignore = "Session 4: in-memory store (SET/GET)"]
fn very_big_payload_in_get_set() {
    let mut c = connect();
    let buf = "abcd".repeat(1_000_000); // 4MB
    assert_eq!(c.cmd(&["SET", "str_big", &buf]), ok());
    assert_eq!(c.cmd(&["GET", "str_big"]), bulk(&buf));
    c.del(&["str_big"]);
}

#[test]
#[ignore = "Session 4: in-memory store (SET/GET)"]
fn set_10000_numeric_keys_and_access_them_in_reverse_order() {
    // 10k keys, read back in reverse. Catches hash-table resize bugs that a
    // handful of keys never would.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..10_000 {
        c.cmd(&["SET", &i.to_string(), &i.to_string()]);
    }
    for i in (0..10_000).rev() {
        assert_eq!(c.cmd(&["GET", &i.to_string()]), bulk(&i.to_string()));
    }
    assert_eq!(c.cmd(&["DBSIZE"]), int(10_000));
    c.cmd(&["FLUSHALL"]);
}

// ---------------------------------------------------------------------------
// Extended SET options
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 5: SET NX option"]
fn extended_set_nx_option() {
    // NX = set only if absent. A refused SET replies nil, not +OK and not an
    // error — that nil is how a lock client knows it did not get the lock.
    let mut c = connect();
    c.del(&["str_nx"]);
    assert_eq!(c.cmd(&["SET", "str_nx", "1", "NX"]), ok());
    assert!(c.cmd(&["SET", "str_nx", "2", "NX"]).is_nil());
    assert_eq!(c.cmd(&["GET", "str_nx"]), bulk("1"));
    c.del(&["str_nx"]);
}

#[test]
#[ignore = "Session 5: SET XX option"]
fn extended_set_xx_option() {
    // XX = set only if present. Mirror image of NX: refused SET replies nil.
    let mut c = connect();
    c.del(&["str_xx"]);
    assert!(c.cmd(&["SET", "str_xx", "1", "XX"]).is_nil());
    assert_eq!(c.cmd(&["EXISTS", "str_xx"]), int(0));
    c.cmd(&["SET", "str_xx", "1"]);
    assert_eq!(c.cmd(&["SET", "str_xx", "2", "XX"]), ok());
    assert_eq!(c.cmd(&["GET", "str_xx"]), bulk("2"));
    c.del(&["str_xx"]);
}

#[test]
#[ignore = "Session 5: SET GET option"]
fn extended_set_get_option() {
    // SET key v2 GET returns the old value "v1" — a swap in one round trip.
    let mut c = connect();
    c.del(&["str_g"]);
    assert!(c.cmd(&["SET", "str_g", "v1", "GET"]).is_nil());
    assert_eq!(c.cmd(&["SET", "str_g", "v2", "GET"]), bulk("v1"));
    assert_eq!(c.cmd(&["GET", "str_g"]), bulk("v2"));
    c.del(&["str_g"]);
}

#[test]
#[ignore = "Session 5: SET GET option combined with NX/XX"]
fn extended_set_get_option_with_nx_and_xx() {
    // The subtle case: NX refuses the write but GET still reports the old value,
    // so the reply is "old", not nil.
    let mut c = connect();
    c.del(&["str_gnx"]);
    c.cmd(&["SET", "str_gnx", "old"]);
    assert_eq!(c.cmd(&["SET", "str_gnx", "new", "NX", "GET"]), bulk("old"));
    assert_eq!(c.cmd(&["GET", "str_gnx"]), bulk("old"), "NX must not write");

    c.del(&["str_gnx"]);
    assert!(c.cmd(&["SET", "str_gnx", "new", "XX", "GET"]).is_nil());
    assert_eq!(c.cmd(&["EXISTS", "str_gnx"]), int(0), "XX must not create");
}

#[test]
#[ignore = "Session 5: SET GET option type checking"]
fn extended_set_get_with_incorrect_type_is_a_wrongtype_error() {
    // Plain SET never type-checks, but SET ... GET must: there is no old string
    // to return when the key holds a list. And it must not write either.
    let mut c = connect();
    c.del(&["str_gwt"]);
    c.cmd(&["RPUSH", "str_gwt", "a"]);
    assert_wrongtype(&c.cmd(&["SET", "str_gwt", "v", "GET"]));
    assert_eq!(c.cmd(&["TYPE", "str_gwt"]), simple("list"), "must not write");
    c.del(&["str_gwt"]);
}

#[test]
#[ignore = "Session 5: SET EX/PX options"]
fn extended_set_ex_and_px_options() {
    // EX is seconds, PX is milliseconds: EX 100 and PX 100000 mean the same TTL.
    let mut c = connect();
    c.del(&["str_ex"]);
    assert_eq!(c.cmd(&["SET", "str_ex", "v", "EX", "100"]), ok());
    let ttl = c.cmd(&["TTL", "str_ex"]).int();
    assert!(ttl > 95 && ttl <= 100, "ttl was {ttl}");

    assert_eq!(c.cmd(&["SET", "str_ex", "v", "PX", "100000"]), ok());
    let ttl = c.cmd(&["TTL", "str_ex"]).int();
    assert!(ttl > 95 && ttl <= 100, "ttl was {ttl}");
    c.del(&["str_ex"]);
}

#[test]
#[ignore = "Session 11: SET EXAT/PXAT options"]
fn extended_set_exat_and_pxat_options() {
    // EXAT takes an absolute unix timestamp, not a duration. This is what makes
    // AOF correct: replaying `EX 100` after a restart would push the deadline out
    // every time, replaying `EXAT <ts>` would not.
    let mut c = connect();
    c.del(&["str_at"]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_eq!(
        c.cmd(&["SET", "str_at", "v", "EXAT", &(now + 100).to_string()]),
        ok()
    );
    let ttl = c.cmd(&["TTL", "str_at"]).int();
    assert!(ttl > 95 && ttl <= 100, "ttl was {ttl}");
    c.del(&["str_at"]);
}

#[test]
#[ignore = "Session 11: SET PXAT with a past time deletes the key"]
fn extended_set_pxat_with_a_past_expiration_time() {
    // PXAT 1 is in the past: not an error, the key is just gone right away.
    let mut c = connect();
    c.del(&["str_past"]);
    assert_eq!(c.cmd(&["SET", "str_past", "v", "PXAT", "1"]), ok());
    assert_eq!(c.cmd(&["EXISTS", "str_past"]), int(0));
}

#[test]
#[ignore = "Session 5: SET syntax errors"]
fn extended_set_can_detect_syntax_errors() {
    // An unknown option, or EX with no number after it, is a syntax error.
    let mut c = connect();
    assert_error(&c.cmd(&["SET", "str_s", "v", "NONSENSE"]), "ERR syntax error");
    assert_error(&c.cmd(&["SET", "str_s", "v", "EX"]), "ERR syntax error");
    assert_error(&c.cmd(&["SET", "str_s", "v", "EX", "notanumber"]), "ERR");
}

#[test]
#[ignore = "Session 5: SET mutually exclusive flags"]
fn extended_set_mutually_exclusive_flags() {
    // NX+XX contradict each other, and so do EX+PX and EX+KEEPTTL. Reject them
    // while parsing instead of picking one.
    let mut c = connect();
    assert_error(&c.cmd(&["SET", "str_s", "v", "NX", "XX"]), "ERR syntax error");
    assert_error(
        &c.cmd(&["SET", "str_s", "v", "EX", "10", "PX", "10000"]),
        "ERR syntax error",
    );
    assert_error(
        &c.cmd(&["SET", "str_s", "v", "EX", "10", "KEEPTTL"]),
        "ERR syntax error",
    );
}

#[test]
#[ignore = "Session 5: SET case-insensitive options"]
fn extended_set_case_insensitive_conditions() {
    // Option names are case-insensitive: "nx" and "ex" work like "NX" and "EX".
    let mut c = connect();
    c.del(&["str_ci"]);
    assert_eq!(c.cmd(&["SET", "str_ci", "v", "nx"]), ok());
    assert_eq!(c.cmd(&["SET", "str_ci", "v2", "xx", "ex", "100"]), ok());
    c.del(&["str_ci"]);
}

#[test]
#[ignore = "Session 5: SET invalid expire time"]
fn extended_set_rejects_non_positive_expire() {
    // EX 0 and EX -1 are errors, not "expire now". The message names the command
    // ("...in 'set' command") — that matters inside MULTI.
    let mut c = connect();
    let reply = c.cmd(&["SET", "str_e", "v", "EX", "0"]);
    assert_error(&reply, "ERR invalid expire time");
    assert!(reply.error().contains("set"));
    assert_error(&c.cmd(&["SET", "str_e", "v", "EX", "-1"]), "ERR invalid expire time");
    assert_error(&c.cmd(&["SET", "str_e", "v", "PX", "0"]), "ERR invalid expire time");
}

#[test]
#[ignore = "Session 11: SET KEEPTTL"]
fn extended_set_keepttl_option() {
    // Plain SET wipes the TTL, so re-setting a value silently makes it permanent.
    // KEEPTTL is how you keep it.
    let mut c = connect();
    c.del(&["str_kt"]);
    c.cmd(&["SET", "str_kt", "v", "EX", "100"]);
    c.cmd(&["SET", "str_kt", "v2"]);
    assert_eq!(c.cmd(&["TTL", "str_kt"]), int(-1), "plain SET clears the TTL");

    c.cmd(&["SET", "str_kt", "v", "EX", "100"]);
    c.cmd(&["SET", "str_kt", "v3", "KEEPTTL"]);
    assert!(c.cmd(&["TTL", "str_kt"]).int() > 90);
    c.del(&["str_kt"]);
}

// ---------------------------------------------------------------------------
// SETNX / SETEX / GETSET / GETDEL / GETEX
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 5: SETNX"]
fn setnx_target_key_missing_and_existing() {
    // SETNX replies 1 when it wrote, 0 when it refused — integers, where
    // SET ... NX replies +OK or nil.
    let mut c = connect();
    c.del(&["str_snx"]);
    assert_eq!(c.cmd(&["SETNX", "str_snx", "foo"]), int(1));
    assert_eq!(c.cmd(&["GET", "str_snx"]), bulk("foo"));
    assert_eq!(c.cmd(&["SETNX", "str_snx", "bar"]), int(0));
    assert_eq!(c.cmd(&["GET", "str_snx"]), bulk("foo"));
    c.del(&["str_snx"]);
}

#[test]
#[ignore = "Session 11: SETNX against an expired volatile key"]
fn setnx_against_expired_volatile_key() {
    // An expired key must look absent even if nothing has swept it out of the map
    // yet. Catches a `map.contains_key()` check that ignores the TTL.
    let mut c = connect();
    c.del(&["str_vol"]);
    c.cmd(&["SET", "str_vol", "old", "PX", "50"]);
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert_eq!(c.cmd(&["SETNX", "str_vol", "new"]), int(1));
    assert_eq!(c.cmd(&["GET", "str_vol"]), bulk("new"));
    c.del(&["str_vol"]);
}

#[test]
#[ignore = "Session 11: SETEX"]
fn setex_basic_and_invalid_seconds() {
    // SETEX key 100 v == SET key v EX 100. Zero and negative seconds are errors.
    let mut c = connect();
    c.del(&["str_sx"]);
    assert_eq!(c.cmd(&["SETEX", "str_sx", "100", "v"]), ok());
    assert_eq!(c.cmd(&["GET", "str_sx"]), bulk("v"));
    assert!(c.cmd(&["TTL", "str_sx"]).int() > 90);
    assert_error(&c.cmd(&["SETEX", "str_sx", "0", "v"]), "ERR invalid expire time");
    assert_error(&c.cmd(&["SETEX", "str_sx", "-1", "v"]), "ERR invalid expire time");
    c.del(&["str_sx"]);
}

#[test]
#[ignore = "Session 5: GETSET"]
fn getset_set_new_value_and_replace_old_value() {
    // The old `SET ... GET`: returns the previous value, nil if there wasn't one.
    let mut c = connect();
    c.del(&["str_gs"]);
    assert!(c.cmd(&["GETSET", "str_gs", "foo"]).is_nil());
    assert_eq!(c.cmd(&["GET", "str_gs"]), bulk("foo"));
    assert_eq!(c.cmd(&["GETSET", "str_gs", "bar"]), bulk("foo"));
    assert_eq!(c.cmd(&["GET", "str_gs"]), bulk("bar"));
    c.del(&["str_gs"]);
}

#[test]
#[ignore = "bonus: GETDEL"]
fn getdel_command() {
    // Read and delete in one step: returns "foo", then the key is gone.
    let mut c = connect();
    c.del(&["str_gd"]);
    assert!(c.cmd(&["GETDEL", "str_gd"]).is_nil());
    c.cmd(&["SET", "str_gd", "foo"]);
    assert_eq!(c.cmd(&["GETDEL", "str_gd"]), bulk("foo"));
    assert_eq!(c.cmd(&["EXISTS", "str_gd"]), int(0));
}

#[test]
#[ignore = "bonus: GETEX"]
fn getex_options() {
    let mut c = connect();
    c.del(&["str_ge"]);
    c.cmd(&["SET", "str_ge", "v"]);

    // No option: a plain GET that leaves the TTL alone.
    assert_eq!(c.cmd(&["GETEX", "str_ge"]), bulk("v"));
    assert_eq!(c.cmd(&["TTL", "str_ge"]), int(-1));

    assert_eq!(c.cmd(&["GETEX", "str_ge", "EX", "100"]), bulk("v"));
    assert!(c.cmd(&["TTL", "str_ge"]).int() > 90);

    // PERSIST removes the TTL while reading; TTL goes back to -1.
    assert_eq!(c.cmd(&["GETEX", "str_ge", "PERSIST"]), bulk("v"));
    assert_eq!(c.cmd(&["TTL", "str_ge"]), int(-1));

    assert_error(&c.cmd(&["GETEX", "str_ge", "NONSENSE"]), "ERR syntax error");
    c.del(&["str_ge"]);
}

// ---------------------------------------------------------------------------
// MGET / MSET / MSETNX
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 5: MGET"]
fn mget() {
    let mut c = connect();
    c.cmd(&["SET", "str_m1", "a"]);
    c.cmd(&["SET", "str_m2", "b"]);
    c.cmd(&["SET", "str_m3", "c"]);
    assert_eq!(
        c.cmd(&["MGET", "str_m1", "str_m2", "str_m3"]),
        bulks(&["a", "b", "c"])
    );
}

#[test]
#[ignore = "Session 5: MGET"]
fn mget_against_non_existing_key() {
    // A missing key becomes a nil element, so the reply has one slot per key.
    // Skipping absent keys would break the client's key-to-value lineup.
    let mut c = connect();
    c.cmd(&["SET", "str_m1", "a"]);
    c.cmd(&["SET", "str_m3", "c"]);
    c.del(&["str_mx"]);
    assert_eq!(
        c.cmd(&["MGET", "str_m1", "str_mx", "str_m3"]),
        arr(vec![bulk("a"), nil(), bulk("c")])
    );
}

#[test]
#[ignore = "Session 5: MGET against a non-string key"]
fn mget_against_non_string_key() {
    // A list in the middle of an MGET gives nil for that slot, not WRONGTYPE.
    // One bad key must not fail the whole read.
    let mut c = connect();
    c.cmd(&["SET", "str_m1", "a"]);
    c.del(&["str_mlist"]);
    c.cmd(&["RPUSH", "str_mlist", "x"]);
    assert_eq!(
        c.cmd(&["MGET", "str_m1", "str_mlist"]),
        arr(vec![bulk("a"), nil()])
    );
    c.del(&["str_mlist"]);
}

#[test]
#[ignore = "Session 5: MSET"]
fn mset_base_case() {
    let mut c = connect();
    assert_eq!(c.cmd(&["MSET", "str_x", "10", "str_y", "foo", "str_z", "bar"]), ok());
    assert_eq!(
        c.cmd(&["MGET", "str_x", "str_y", "str_z"]),
        bulks(&["10", "foo", "bar"])
    );
}

#[test]
#[ignore = "Session 5: MSET with a repeated key"]
fn mset_with_already_existing_same_key_twice() {
    // MSET k a k b: last write wins, still +OK. Not an error.
    let mut c = connect();
    c.del(&["str_dup"]);
    assert_eq!(c.cmd(&["MSET", "str_dup", "a", "str_dup", "b"]), ok());
    assert_eq!(c.cmd(&["GET", "str_dup"]), bulk("b"));
    c.del(&["str_dup"]);
}

#[test]
#[ignore = "Session 5: MSET/MSETNX arity"]
fn mset_and_msetnx_wrong_number_of_args() {
    // An odd number of args means a key with no value. Reject before writing
    // anything, or you get a half-applied MSET.
    let mut c = connect();
    assert_error(&c.cmd(&["MSET", "str_x", "10", "str_y"]), "ERR wrong number");
    assert_error(&c.cmd(&["MSETNX", "str_x", "10", "str_y"]), "ERR wrong number");
}

#[test]
#[ignore = "Session 5: MSETNX"]
fn msetnx_with_not_existing_keys() {
    let mut c = connect();
    c.del(&["str_n1", "str_n2"]);
    assert_eq!(c.cmd(&["MSETNX", "str_n1", "x", "str_n2", "y"]), int(1));
    assert_eq!(c.cmd(&["MGET", "str_n1", "str_n2"]), bulks(&["x", "y"]));
    c.del(&["str_n1", "str_n2"]);
}

#[test]
#[ignore = "Session 5: MSETNX is all-or-nothing"]
fn msetnx_with_already_existent_key() {
    // All-or-nothing: one existing key means none of the three are written. A
    // loop of SETNX calls would write the first key before finding out.
    let mut c = connect();
    c.del(&["str_n1", "str_n2", "str_n3"]);
    c.cmd(&["SET", "str_n2", "existing"]);
    assert_eq!(
        c.cmd(&["MSETNX", "str_n1", "x", "str_n2", "y", "str_n3", "z"]),
        int(0)
    );
    assert_eq!(c.cmd(&["EXISTS", "str_n1"]), int(0));
    assert_eq!(c.cmd(&["GET", "str_n2"]), bulk("existing"));
    assert_eq!(c.cmd(&["EXISTS", "str_n3"]), int(0));
    c.del(&["str_n1", "str_n2", "str_n3"]);
}

// ---------------------------------------------------------------------------
// APPEND / STRLEN
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 5: APPEND"]
fn append_basic_usage() {
    // APPEND creates the key if missing, and returns the total length, not the
    // length appended: "foo" then "bar" gives 3 then 6.
    let mut c = connect();
    c.del(&["str_ap"]);
    assert_eq!(c.cmd(&["APPEND", "str_ap", "foo"]), int(3));
    assert_eq!(c.cmd(&["APPEND", "str_ap", "bar"]), int(6));
    assert_eq!(c.cmd(&["GET", "str_ap"]), bulk("foobar"));
    c.del(&["str_ap"]);
}

#[test]
#[ignore = "Session 5: APPEND"]
fn append_with_empty_string() {
    // Appending "" still creates the key — an empty string is a real value here.
    let mut c = connect();
    c.del(&["str_ap"]);
    assert_eq!(c.cmd(&["APPEND", "str_ap", ""]), int(0));
    assert_eq!(c.cmd(&["EXISTS", "str_ap"]), int(1), "APPEND creates the key");
    assert_eq!(c.cmd(&["GET", "str_ap"]), bulk(""));
    c.del(&["str_ap"]);
}

#[test]
#[ignore = "Session 5: STRLEN"]
fn strlen_against_plain_string_and_missing_key() {
    // A missing key has length 0, same as an empty string. No error.
    let mut c = connect();
    c.cmd(&["SET", "str_sl", "Hello World"]);
    assert_eq!(c.cmd(&["STRLEN", "str_sl"]), int(11));
    c.del(&["str_slx"]);
    assert_eq!(c.cmd(&["STRLEN", "str_slx"]), int(0));
}

#[test]
#[ignore = "Session 5: STRLEN counts bytes, not characters"]
fn strlen_counts_bytes() {
    // STRLEN counts bytes, not characters: "é" is 1 char but 2 bytes, so 2.
    // Store values as Vec<u8> and use .len(); chars().count() fails here.
    let mut c = connect();
    c.cmd(&["SET", "str_utf", "é"]);
    assert_eq!(c.cmd(&["STRLEN", "str_utf"]), int(2));
    c.del(&["str_utf"]);
}

// ---------------------------------------------------------------------------
// GETRANGE / SETRANGE
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: GETRANGE"]
fn getrange_against_string_value() {
    // Both ends are inclusive, unlike Rust's `&s[0..4]`: 0,4 of "Hello World" is
    // "Hello". Negative indices count from the end, and -1 means the last byte.
    let mut c = connect();
    c.cmd(&["SET", "str_gr", "Hello World"]);
    assert_eq!(c.cmd(&["GETRANGE", "str_gr", "0", "4"]), bulk("Hello"));
    assert_eq!(c.cmd(&["GETRANGE", "str_gr", "0", "-1"]), bulk("Hello World"));
    assert_eq!(c.cmd(&["GETRANGE", "str_gr", "-5", "-1"]), bulk("World"));
    assert_eq!(c.cmd(&["GETRANGE", "str_gr", "0", "0"]), bulk("H"));
    // start > end gives "", not an error.
    assert_eq!(c.cmd(&["GETRANGE", "str_gr", "5", "3"]), bulk(""));
    // Past the end clamps to the length; no panic.
    assert_eq!(c.cmd(&["GETRANGE", "str_gr", "0", "10000"]), bulk("Hello World"));
    c.del(&["str_gr"]);
}

#[test]
#[ignore = "bonus: GETRANGE"]
fn getrange_against_non_existing_key() {
    // A missing key reads as "" rather than nil.
    let mut c = connect();
    c.del(&["str_grx"]);
    assert_eq!(c.cmd(&["GETRANGE", "str_grx", "0", "-1"]), bulk(""));
}

#[test]
#[ignore = "bonus: GETRANGE with huge ranges (github issue #1844)"]
fn getrange_with_huge_ranges() {
    // 4294967297 is way past the end. Clamp to the string length *before* doing
    // `end - start`, or the subtraction overflows (redis issue #1844).
    let mut c = connect();
    c.cmd(&["SET", "str_gr", "Hello World"]);
    assert_eq!(
        c.cmd(&["GETRANGE", "str_gr", "0", "4294967297"]),
        bulk("Hello World")
    );
    assert_eq!(c.cmd(&["GETRANGE", "str_gr", "-4294967297", "-1"]), bulk("Hello World"));
    c.del(&["str_gr"]);
}

#[test]
#[ignore = "bonus: SETRANGE"]
fn setrange_against_non_existing_key() {
    // SETRANGE at offset 5 on a missing key pads the gap with NUL bytes:
    // "\0\0\0\0\0Redis", length 10. Writing "" creates no key at all.
    let mut c = connect();
    c.del(&["str_sr"]);
    assert_eq!(c.cmd(&["SETRANGE", "str_sr", "0", "Redis"]), int(5));
    assert_eq!(c.cmd(&["GET", "str_sr"]), bulk("Redis"));

    c.del(&["str_sr"]);
    assert_eq!(c.cmd(&["SETRANGE", "str_sr", "0", ""]), int(0));
    assert_eq!(c.cmd(&["EXISTS", "str_sr"]), int(0));

    c.del(&["str_sr"]);
    assert_eq!(c.cmd(&["SETRANGE", "str_sr", "5", "Redis"]), int(10));
    assert_eq!(
        c.cmd(&["GET", "str_sr"]),
        Reply::Bulk(b"\x00\x00\x00\x00\x00Redis".to_vec())
    );
    c.del(&["str_sr"]);
}

#[test]
#[ignore = "bonus: SETRANGE"]
fn setrange_against_existing_key() {
    // Overwrite in place: "Hello World" at offset 6 with "Redis" is "Hello Redis".
    let mut c = connect();
    c.cmd(&["SET", "str_sr", "Hello World"]);
    assert_eq!(c.cmd(&["SETRANGE", "str_sr", "6", "Redis"]), int(11));
    assert_eq!(c.cmd(&["GET", "str_sr"]), bulk("Hello Redis"));

    // A write running past the end extends the string instead of truncating.
    c.cmd(&["SET", "str_sr", "Hello World"]);
    assert_eq!(c.cmd(&["SETRANGE", "str_sr", "6", "Redis World"]), int(17));
    assert_eq!(c.cmd(&["GET", "str_sr"]), bulk("Hello Redis World"));
    c.del(&["str_sr"]);
}

#[test]
#[ignore = "bonus: SETRANGE bounds checking"]
fn setrange_with_out_of_range_offset() {
    // Negative offsets are errors, and so is any write past the 512MB string
    // limit — check the size first, don't try to allocate it.
    let mut c = connect();
    c.del(&["str_sr"]);
    assert_error(&c.cmd(&["SETRANGE", "str_sr", "-1", "x"]), "ERR offset is out of range");
    assert_error(
        &c.cmd(&["SETRANGE", "str_sr", "536870911", "xxx"]),
        "ERR string exceeds maximum allowed size",
    );
}

#[test]
#[ignore = "bonus: SUBSTR (deprecated alias for GETRANGE)"]
fn coverage_substr() {
    // SUBSTR is just an old name for GETRANGE — same code, two names.
    let mut c = connect();
    c.cmd(&["SET", "str_sub", "Hello World"]);
    assert_eq!(c.cmd(&["SUBSTR", "str_sub", "0", "4"]), bulk("Hello"));
    assert_eq!(c.cmd(&["SUBSTR", "str_sub", "0", "-1"]), bulk("Hello World"));
    c.del(&["str_sub"]);
}
