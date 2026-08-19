//! Ported from `redis/tests/unit/expire.tcl`.
//!
//! EXPIRE, TTL, PERSIST and friends. One idea runs through the whole file: a key
//! past its deadline must look gone to every command right away, even if it is
//! still sitting in the HashMap waiting for the sweep.
//!
//! Skipped from the original: replication, and the `DEBUG SET-ACTIVE-EXPIRE` /
//! `DEBUG SLEEP` tests.
//!
//! Real sleeps here, so this file is slow.

mod common;
use common::*;
use std::thread::sleep;
use std::time::Duration;

// ---------------------------------------------------------------------------
// EXPIRE / EXPIREAT basics
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 6: EXPIRE"]
fn expire_set_timeouts_multiple_times() {
    // A second EXPIRE replaces the deadline, it does not add to it:
    // `EXPIRE k 100` then `EXPIRE k 200` => TTL is ~200, not 300.
    // Returns 1 if a TTL was set, 0 if the key is missing.
    let mut c = connect();
    c.del(&["exp_x"]);
    c.cmd(&["SET", "exp_x", "somevalue"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_x", "100"]), int(1));
    assert_eq!(c.cmd(&["EXPIRE", "exp_x", "200"]), int(1));
    let ttl = c.cmd(&["TTL", "exp_x"]).int();
    assert!(ttl > 190 && ttl <= 200, "ttl was {ttl}");

    c.del(&["exp_missing"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_missing", "100"]), int(0));
}

#[test]
#[ignore = "Session 11: expiry actually removes the key"]
fn expire_key_should_no_longer_be_here_after_the_deadline() {
    // The basic one: SET, EXPIRE 1, wait 1.6s, and GET must be nil.
    let mut c = connect();
    c.del(&["exp_x"]);
    c.cmd(&["SET", "exp_x", "somevalue"]);
    c.cmd(&["EXPIRE", "exp_x", "1"]);
    assert_eq!(c.cmd(&["GET", "exp_x"]), bulk("somevalue"));
    sleep(Duration::from_millis(1600));
    assert!(c.cmd(&["GET", "exp_x"]).is_nil());
    assert_eq!(c.cmd(&["EXISTS", "exp_x"]), int(0));
}

#[test]
#[ignore = "Session 11: writing to a volatile key"]
fn expire_write_on_expire_should_work() {
    // A TTL does not make a key read-only. APPEND on a key with a TTL keeps
    // both the new value and the deadline: "a" + "b" => "ab", TTL still ~100.
    let mut c = connect();
    c.del(&["exp_w"]);
    c.cmd(&["SET", "exp_w", "a"]);
    c.cmd(&["EXPIRE", "exp_w", "100"]);
    c.cmd(&["APPEND", "exp_w", "b"]);
    assert_eq!(c.cmd(&["GET", "exp_w"]), bulk("ab"));
    assert!(c.cmd(&["TTL", "exp_w"]).int() > 90);
    c.del(&["exp_w"]);
}

#[test]
#[ignore = "Session 6: EXPIREAT"]
fn expireat_check_for_expire_alike_behavior() {
    // EXPIREAT takes an absolute unix time in seconds. `EXPIREAT k 1` (1970) is
    // already past, so the key dies at once -- but it still returns 1, because
    // the command itself worked.
    let mut c = connect();
    c.del(&["exp_a"]);
    c.cmd(&["SET", "exp_a", "v"]);
    assert_eq!(c.cmd(&["EXPIREAT", "exp_a", "1"]), int(1));
    assert_eq!(c.cmd(&["EXISTS", "exp_a"]), int(0));
}

#[test]
#[ignore = "Session 6: EXPIRE with a past/negative TTL deletes the key"]
fn expire_with_negative_ttl_deletes_the_key() {
    // `EXPIRE k -1` => key gone, returns 1.
    let mut c = connect();
    c.del(&["exp_n"]);
    c.cmd(&["SET", "exp_n", "v"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_n", "-1"]), int(1));
    assert_eq!(c.cmd(&["EXISTS", "exp_n"]), int(0));
}

// ---------------------------------------------------------------------------
// TTL / PTTL / PERSIST
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 6: TTL sentinel values"]
fn ttl_returns_minus_one_and_minus_two() {
    // TTL sentinels: -1 = key exists but has no expiry, -2 = key does not exist.
    // Easy to swap, and nothing complains until a client misbehaves.
    let mut c = connect();
    c.del(&["exp_t"]);
    c.cmd(&["SET", "exp_t", "v"]);
    assert_eq!(c.cmd(&["TTL", "exp_t"]), int(-1));
    assert_eq!(c.cmd(&["PTTL", "exp_t"]), int(-1));

    c.del(&["exp_gone"]);
    assert_eq!(c.cmd(&["TTL", "exp_gone"]), int(-2));
    assert_eq!(c.cmd(&["PTTL", "exp_gone"]), int(-2));
}

#[test]
#[ignore = "Session 6: TTL / PTTL"]
fn ttl_returns_time_to_live_in_seconds_and_milliseconds() {
    // TTL rounds up, so `EXPIRE k 10` then `TTL k` reads 10, never 9.
    // PTTL is the same deadline in milliseconds (~10000).
    let mut c = connect();
    c.del(&["exp_t"]);
    c.cmd(&["SET", "exp_t", "v"]);
    c.cmd(&["EXPIRE", "exp_t", "10"]);
    let ttl = c.cmd(&["TTL", "exp_t"]).int();
    assert!(ttl > 8 && ttl <= 10, "ttl was {ttl}");
    let pttl = c.cmd(&["PTTL", "exp_t"]).int();
    assert!(pttl > 8000 && pttl <= 10000, "pttl was {pttl}");
    c.del(&["exp_t"]);
}

#[test]
#[ignore = "Session 6: PERSIST"]
fn persist_can_undo_an_expire() {
    // PERSIST drops the deadline and keeps the value: TTL goes 50 -> -1.
    let mut c = connect();
    c.del(&["exp_p"]);
    c.cmd(&["SET", "exp_p", "v"]);
    c.cmd(&["EXPIRE", "exp_p", "50"]);
    assert!(c.cmd(&["TTL", "exp_p"]).int() > 40);
    assert_eq!(c.cmd(&["PERSIST", "exp_p"]), int(1));
    assert_eq!(c.cmd(&["TTL", "exp_p"]), int(-1));
    assert_eq!(c.cmd(&["GET", "exp_p"]), bulk("v"));
    c.del(&["exp_p"]);
}

#[test]
#[ignore = "Session 6: PERSIST return values"]
fn persist_returns_zero_against_non_existing_or_non_volatile_keys() {
    // PERSIST returns 1 only when it actually removed a TTL. No key => 0,
    // key with no TTL => 0.
    let mut c = connect();
    c.del(&["exp_p"]);
    assert_eq!(c.cmd(&["PERSIST", "exp_p"]), int(0));
    c.cmd(&["SET", "exp_p", "v"]);
    assert_eq!(c.cmd(&["PERSIST", "exp_p"]), int(0));
    c.del(&["exp_p"]);
}

#[test]
#[ignore = "Session 11: SET clears an existing TTL"]
fn set_command_will_remove_expire() {
    // Plain `SET k v` wipes an existing TTL: TTL goes 100 -> -1. Easy to miss,
    // because SET looks like it only touches the value.
    let mut c = connect();
    c.del(&["exp_s"]);
    c.cmd(&["SET", "exp_s", "v", "EX", "100"]);
    assert!(c.cmd(&["TTL", "exp_s"]).int() > 90);
    c.cmd(&["SET", "exp_s", "v2"]);
    assert_eq!(c.cmd(&["TTL", "exp_s"]), int(-1));
    c.del(&["exp_s"]);
}

#[test]
#[ignore = "Session 11: commands that preserve vs clear the TTL"]
fn ttl_survives_value_mutation_but_not_replacement() {
    // Rule: changing a value keeps the TTL (APPEND, INCR, RPUSH); replacing the
    // key clears it (SET, GETSET). Each command decides for itself, so this is
    // easy to get inconsistent.
    let mut c = connect();

    c.del(&["exp_m"]);
    c.cmd(&["SET", "exp_m", "a", "EX", "100"]);
    c.cmd(&["APPEND", "exp_m", "b"]);
    assert!(c.cmd(&["TTL", "exp_m"]).int() > 90, "APPEND keeps the TTL");

    c.del(&["exp_m"]);
    c.cmd(&["SET", "exp_m", "1", "EX", "100"]);
    c.cmd(&["INCR", "exp_m"]);
    assert!(c.cmd(&["TTL", "exp_m"]).int() > 90, "INCR keeps the TTL");

    c.del(&["exp_m"]);
    c.cmd(&["RPUSH", "exp_m", "a"]);
    c.cmd(&["EXPIRE", "exp_m", "100"]);
    c.cmd(&["RPUSH", "exp_m", "b"]);
    assert!(c.cmd(&["TTL", "exp_m"]).int() > 90, "RPUSH keeps the TTL");

    c.del(&["exp_m"]);
    c.cmd(&["SET", "exp_m", "v", "EX", "100"]);
    c.cmd(&["GETSET", "exp_m", "v2"]);
    assert_eq!(c.cmd(&["TTL", "exp_m"]), int(-1), "GETSET clears the TTL");
    c.del(&["exp_m"]);
}

// ---------------------------------------------------------------------------
// Millisecond precision
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 11: millisecond expiry precision"]
fn expire_precision_is_now_the_millisecond() {
    // `PEXPIRE k 100`: alive at 50ms, gone at 250ms. Store deadlines in
    // milliseconds even though EXPIRE speaks seconds -- storing seconds means
    // rewriting the stored form later to get sub-second TTLs.
    let mut c = connect();
    c.del(&["exp_ms"]);
    c.cmd(&["SET", "exp_ms", "v"]);
    c.cmd(&["PEXPIRE", "exp_ms", "100"]);
    sleep(Duration::from_millis(50));
    assert_eq!(c.cmd(&["GET", "exp_ms"]), bulk("v"), "must still be alive at 50ms");
    sleep(Duration::from_millis(200));
    assert!(c.cmd(&["GET", "exp_ms"]).is_nil(), "must be gone at 250ms");
}

#[test]
#[ignore = "Session 11: PSETEX / PEXPIRE / PEXPIREAT"]
fn psetex_pexpire_and_pexpireat_can_set_sub_second_expires() {
    // Three ways to spell a 100ms TTL: PSETEX (set + TTL), PEXPIRE (relative ms),
    // PEXPIREAT (absolute ms). All three keys must be gone at 200ms.
    let mut c = connect();

    c.del(&["exp_a"]);
    c.cmd(&["PSETEX", "exp_a", "100", "v"]);
    assert_eq!(c.cmd(&["GET", "exp_a"]), bulk("v"));
    sleep(Duration::from_millis(200));
    assert!(c.cmd(&["GET", "exp_a"]).is_nil());

    c.del(&["exp_b"]);
    c.cmd(&["SET", "exp_b", "v"]);
    c.cmd(&["PEXPIRE", "exp_b", "100"]);
    sleep(Duration::from_millis(200));
    assert!(c.cmd(&["GET", "exp_b"]).is_nil());

    c.del(&["exp_c"]);
    c.cmd(&["SET", "exp_c", "v"]);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    c.cmd(&["PEXPIREAT", "exp_c", &(now_ms + 100).to_string()]);
    sleep(Duration::from_millis(200));
    assert!(c.cmd(&["GET", "exp_c"]).is_nil());
}

#[test]
#[ignore = "bonus: EXPIRETIME / PEXPIRETIME"]
fn expiretime_returns_the_absolute_expiration_time() {
    // EXPIRETIME gives back the deadline itself, not the remaining time.
    // It only works if you store absolute timestamps, not durations.
    let mut c = connect();
    c.del(&["exp_et"]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    c.cmd(&["SET", "exp_et", "v"]);
    c.cmd(&["EXPIREAT", "exp_et", &(now + 100).to_string()]);
    assert_eq!(c.cmd(&["EXPIRETIME", "exp_et"]), int(now + 100));
    assert_eq!(c.cmd(&["PEXPIRETIME", "exp_et"]), int((now + 100) * 1000));

    // Same -1 / -2 sentinels as TTL.
    c.cmd(&["SET", "exp_et2", "v"]);
    assert_eq!(c.cmd(&["EXPIRETIME", "exp_et2"]), int(-1));
    c.del(&["exp_et3"]);
    assert_eq!(c.cmd(&["EXPIRETIME", "exp_et3"]), int(-2));
    c.del(&["exp_et", "exp_et2"]);
}

// ---------------------------------------------------------------------------
// Lazy and active expiry
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 11: lazy expiry on read"]
fn redis_should_lazy_expire_keys() {
    // Lazy expiry: a dead key is dropped when a command touches it. Get this
    // half right first -- the background sweep is only an optimisation on top.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..3 {
        c.cmd(&["SET", &format!("exp_lazy{i}"), "v", "PX", "50"]);
    }
    sleep(Duration::from_millis(150));
    for i in 0..3 {
        assert!(c.cmd(&["GET", &format!("exp_lazy{i}")]).is_nil());
    }
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 11: active expiry sweep"]
fn redis_should_actively_expire_keys_incrementally() {
    // Active expiry: keys nobody reads again must still be freed, or a cache of
    // one-shot keys leaks memory. Nothing here touches the keys, only DBSIZE, so
    // lazy expiry alone leaves DBSIZE at 500 and this fails.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..500 {
        c.cmd(&["SET", &format!("exp_act{i}"), "v", "PX", "100"]);
    }
    assert_eq!(c.cmd(&["DBSIZE"]), int(500));
    sleep(Duration::from_millis(1500));
    assert_eq!(c.cmd(&["DBSIZE"]), int(0));
}

#[test]
#[ignore = "Session 11: expiry must not resurrect keys (issue #1026)"]
fn expire_should_not_resurrect_keys() {
    // EXPIRE on a key that already died must return 0 and leave it dead.
    // Setting the new TTL before checking whether the key is alive brings the
    // old value back from the dead.
    let mut c = connect();
    c.del(&["exp_res"]);
    c.cmd(&["SET", "exp_res", "v", "PX", "50"]);
    sleep(Duration::from_millis(150));
    assert_eq!(c.cmd(&["EXPIRE", "exp_res", "1000"]), int(0));
    assert_eq!(c.cmd(&["EXISTS", "exp_res"]), int(0));
    assert!(c.cmd(&["GET", "exp_res"]).is_nil());
}

#[test]
#[ignore = "Session 11: expired keys must be invisible to every command"]
fn expired_keys_are_invisible_to_all_commands() {
    // GET, EXISTS, TYPE, TTL, STRLEN, DEL, KEYS, DBSIZE and MGET must all agree
    // a stale key is gone. This is the argument for one TTL check inside
    // `Db::get`, not one per command.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    c.cmd(&["SET", "exp_inv", "v", "PX", "50"]);
    c.cmd(&["SET", "exp_alive", "v"]);
    sleep(Duration::from_millis(150));

    assert!(c.cmd(&["GET", "exp_inv"]).is_nil());
    assert_eq!(c.cmd(&["EXISTS", "exp_inv"]), int(0));
    assert_eq!(c.cmd(&["TYPE", "exp_inv"]), simple("none"));
    assert_eq!(c.cmd(&["TTL", "exp_inv"]), int(-2));
    assert_eq!(c.cmd(&["STRLEN", "exp_inv"]), int(0));
    assert_eq!(c.cmd(&["DEL", "exp_inv"]), int(0));
    assert_eq!(c.cmd(&["KEYS", "*"]), bulks(&["exp_alive"]));
    assert_eq!(c.cmd(&["DBSIZE"]), int(1));
    assert_eq!(c.cmd(&["MGET", "exp_inv"]), arr(vec![nil()]));
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 11: 5 keys in, 5 keys out"]
fn five_keys_in_five_keys_out() {
    // Sanity check from the TCL suite: 5 keys set, one with a long TTL, and
    // KEYS * still lists all 5.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    c.cmd(&["SET", "exp_a", "1"]);
    c.cmd(&["SET", "exp_b", "2"]);
    c.cmd(&["SET", "exp_c", "3"]);
    c.cmd(&["EXPIRE", "exp_c", "100"]);
    c.cmd(&["SET", "exp_d", "4"]);
    c.cmd(&["SET", "exp_e", "5"]);
    assert_eq!(
        c.cmd(&["KEYS", "*"]).sorted(),
        vec!["exp_a", "exp_b", "exp_c", "exp_d", "exp_e"]
    );
    c.cmd(&["FLUSHALL"]);
}

// ---------------------------------------------------------------------------
// Argument validation and overflow
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 6: EXPIRE argument validation"]
fn expire_with_empty_string_as_ttl_should_report_an_error() {
    // `EXPIRE k ""` and `EXPIRE k 1.5` are both "not an integer" -- no floats.
    let mut c = connect();
    c.cmd(&["SET", "exp_v", "v"]);
    assert_error(&c.cmd(&["EXPIRE", "exp_v", ""]), "ERR value is not an integer");
    assert_error(&c.cmd(&["EXPIRE", "exp_v", "1.5"]), "ERR value is not an integer");
    c.del(&["exp_v"]);
}

#[test]
#[ignore = "Session 11: EXPIRE overflow when converted to milliseconds"]
fn expire_with_big_integer_overflows_when_converted_to_milliseconds() {
    // EXPIRE computes `now_ms + seconds * 1000`; with i64::MAX both the multiply
    // and the add overflow. Use checked arithmetic and return the error -- a
    // wrapped result lands in the past and deletes the key instead.
    let mut c = connect();
    c.cmd(&["SET", "exp_v", "v"]);
    assert_error(
        &c.cmd(&["EXPIRE", "exp_v", "9223372036854775807"]),
        "ERR invalid expire time",
    );
    assert_error(
        &c.cmd(&["PEXPIRE", "exp_v", "9223372036854775807"]),
        "ERR invalid expire time",
    );
    assert_error(
        &c.cmd(&["SET", "exp_v", "v", "EX", "9223372036854775807"]),
        "ERR invalid expire time",
    );
    c.del(&["exp_v"]);
}

#[test]
#[ignore = "Session 11: EXPIRE with big negative integer"]
fn expire_with_big_negative_integer() {
    // A huge negative EXPIRE is an error (it underflows the add).
    let mut c = connect();
    c.cmd(&["SET", "exp_v", "v"]);
    assert_error(
        &c.cmd(&["EXPIRE", "exp_v", "-9223372036854775808"]),
        "ERR invalid expire time",
    );
    // PEXPIREAT is an absolute time, so the same number is just a deadline long
    // past: legal, returns 1, deletes the key.
    assert_eq!(c.cmd(&["PEXPIREAT", "exp_v", "-9223372036854775808"]), int(1));
    assert_eq!(c.cmd(&["EXISTS", "exp_v"]), int(0));
}

// ---------------------------------------------------------------------------
// EXPIRE NX / XX / GT / LT
// ---------------------------------------------------------------------------
//
// Redis 7 conditional TTL updates. The point: "extend a session lease but never
// shorten it" done as TTL-then-EXPIRE has a race; `EXPIRE k n GT` does not.
// A key with no TTL counts as infinity, which is why GT never applies to it and
// LT always does.

#[test]
#[ignore = "bonus: EXPIRE NX option"]
fn expire_with_nx_option() {
    // NX = set a TTL only if there is not one yet. Second NX returns 0 and the
    // first TTL (100) survives.
    let mut c = connect();
    c.del(&["exp_o"]);
    c.cmd(&["SET", "exp_o", "v"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "100", "NX"]), int(1));
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "200", "NX"]), int(0));
    let ttl = c.cmd(&["TTL", "exp_o"]).int();
    assert!(ttl > 90 && ttl <= 100, "ttl was {ttl}");
    c.del(&["exp_o"]);
}

#[test]
#[ignore = "bonus: EXPIRE XX option"]
fn expire_with_xx_option() {
    // XX = update only a TTL that already exists. On a key with no TTL it
    // returns 0 and changes nothing.
    let mut c = connect();
    c.del(&["exp_o"]);
    c.cmd(&["SET", "exp_o", "v"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "100", "XX"]), int(0));
    assert_eq!(c.cmd(&["TTL", "exp_o"]), int(-1));
    c.cmd(&["EXPIRE", "exp_o", "100"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "200", "XX"]), int(1));
    assert!(c.cmd(&["TTL", "exp_o"]).int() > 190);
    c.del(&["exp_o"]);
}

#[test]
#[ignore = "bonus: EXPIRE GT option"]
fn expire_with_gt_option() {
    // GT = only push the deadline further out. 100 -> 200 works, 200 -> 50 does
    // not. On a key with no TTL, GT always returns 0 (no TTL = infinity).
    let mut c = connect();
    c.del(&["exp_o"]);
    c.cmd(&["SET", "exp_o", "v"]);
    c.cmd(&["EXPIRE", "exp_o", "100"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "200", "GT"]), int(1));
    assert!(c.cmd(&["TTL", "exp_o"]).int() > 190);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "50", "GT"]), int(0));
    assert!(c.cmd(&["TTL", "exp_o"]).int() > 190);

    c.del(&["exp_o"]);
    c.cmd(&["SET", "exp_o", "v"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "100", "GT"]), int(0));
    assert_eq!(c.cmd(&["TTL", "exp_o"]), int(-1));
    c.del(&["exp_o"]);
}

#[test]
#[ignore = "bonus: EXPIRE LT option"]
fn expire_with_lt_option() {
    // LT = only pull the deadline in. 100 -> 50 works, 50 -> 200 does not.
    // On a key with no TTL, LT always applies (infinity beats any number).
    let mut c = connect();
    c.del(&["exp_o"]);
    c.cmd(&["SET", "exp_o", "v"]);
    c.cmd(&["EXPIRE", "exp_o", "100"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "50", "LT"]), int(1));
    let ttl = c.cmd(&["TTL", "exp_o"]).int();
    assert!(ttl > 40 && ttl <= 50, "ttl was {ttl}");
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "200", "LT"]), int(0));

    c.del(&["exp_o"]);
    c.cmd(&["SET", "exp_o", "v"]);
    assert_eq!(c.cmd(&["EXPIRE", "exp_o", "100", "LT"]), int(1));
    c.del(&["exp_o"]);
}

#[test]
#[ignore = "bonus: EXPIRE option validation"]
fn expire_with_conflicting_options() {
    // NX cannot pair with XX/GT/LT, and GT cannot pair with LT. Unknown options
    // are an error too, not something to ignore.
    let mut c = connect();
    c.cmd(&["SET", "exp_o", "v"]);
    assert_error(&c.cmd(&["EXPIRE", "exp_o", "100", "NX", "XX"]), "ERR");
    assert_error(&c.cmd(&["EXPIRE", "exp_o", "100", "NX", "GT"]), "ERR");
    assert_error(&c.cmd(&["EXPIRE", "exp_o", "100", "GT", "LT"]), "ERR");
    assert_error(&c.cmd(&["EXPIRE", "exp_o", "100", "AB"]), "ERR Unsupported option");
    c.del(&["exp_o"]);
}
