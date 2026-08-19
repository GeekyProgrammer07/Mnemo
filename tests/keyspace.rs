//! Ported from `redis/tests/unit/keyspace.tcl`.
//!
//! Commands that work on keys rather than values: DEL, EXISTS, TYPE, KEYS,
//! RENAME, COPY, DBSIZE.
//!
//! Skipped from the original: multi-database (SELECT, MOVE, SWAPDB,
//! `COPY ... DB n`) and `DEBUG DIGEST`. COPY within one database is kept.
//!
//! Keys are prefixed (`ks_*`, `kp_*`, ...) because every test shares one server.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// DEL / EXISTS
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 4: in-memory store (DEL)"]
fn del_against_a_single_item() {
    // SET then DEL, and GET must be nil afterwards.
    let mut c = connect();
    c.cmd(&["SET", "ks_x", "foo"]);
    assert_eq!(c.cmd(&["GET", "ks_x"]), bulk("foo"));
    c.cmd(&["DEL", "ks_x"]);
    assert!(c.cmd(&["GET", "ks_x"]).is_nil());
}

#[test]
#[ignore = "Session 4: in-memory store (DEL)"]
fn vararg_del() {
    // DEL counts what it removed, not what you named: 3 real keys + 1 missing
    // => 3. That is how a client tells "deleted" from "was not there".
    let mut c = connect();
    c.cmd(&["SET", "ks_foo1", "a"]);
    c.cmd(&["SET", "ks_foo2", "b"]);
    c.cmd(&["SET", "ks_foo3", "c"]);
    assert_eq!(
        c.cmd(&["DEL", "ks_foo1", "ks_foo2", "ks_foo3", "ks_foo4"]),
        int(3)
    );
    assert_eq!(
        c.cmd(&["MGET", "ks_foo1", "ks_foo2", "ks_foo3"]),
        arr(vec![nil(), nil(), nil()])
    );
}

#[test]
#[ignore = "Session 4: in-memory store (DEL)"]
fn del_against_non_existing_key() {
    // Deleting a key that is not there is 0, not an error.
    let mut c = connect();
    c.del(&["ks_nokey"]);
    assert_eq!(c.cmd(&["DEL", "ks_nokey"]), int(0));
}

#[test]
#[ignore = "Session 4: in-memory store (EXISTS)"]
fn exists() {
    // 1 while the key is there, 0 once it is deleted.
    let mut c = connect();
    c.cmd(&["SET", "ks_newkey", "test"]);
    assert_eq!(c.cmd(&["EXISTS", "ks_newkey"]), int(1));
    c.cmd(&["DEL", "ks_newkey"]);
    assert_eq!(c.cmd(&["EXISTS", "ks_newkey"]), int(0));
}

#[test]
#[ignore = "Session 4: in-memory store (EXISTS)"]
fn exists_with_multiple_keys_counts_duplicates() {
    // EXISTS counts arguments, not distinct keys:
    // `EXISTS k k k` on one real key => 3.
    let mut c = connect();
    c.cmd(&["MSET", "ks_e1", "a", "ks_e2", "b"]);
    c.del(&["ks_e3"]);
    assert_eq!(c.cmd(&["EXISTS", "ks_e1", "ks_e2", "ks_e3"]), int(2));
    assert_eq!(c.cmd(&["EXISTS", "ks_e1", "ks_e1", "ks_e1"]), int(3));
}

#[test]
#[ignore = "Session 4: in-memory store (SET/GET/EXISTS)"]
fn zero_length_value_in_key() {
    // The empty string is a real value: GET sends `$0\r\n\r\n`, not `$-1\r\n`,
    // and EXISTS says 1. Modelling "missing" as "empty" fails right here.
    let mut c = connect();
    c.cmd(&["SET", "ks_emptykey", ""]);
    assert_eq!(c.cmd(&["GET", "ks_emptykey"]), bulk(""));
    assert_eq!(c.cmd(&["EXISTS", "ks_emptykey"]), int(1));
    c.cmd(&["DEL", "ks_emptykey"]);
    assert_eq!(c.cmd(&["EXISTS", "ks_emptykey"]), int(0));
}

// ---------------------------------------------------------------------------
// TYPE
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 4: in-memory store (TYPE)"]
fn type_against_a_missing_key() {
    // A missing key's type is the simple string `+none\r\n` -- not nil, not an error.
    let mut c = connect();
    c.del(&["ks_missing"]);
    assert_eq!(c.cmd(&["TYPE", "ks_missing"]), simple("none"));
}

#[test]
#[ignore = "Session 4: in-memory store (TYPE)"]
fn type_reports_every_value_kind() {
    // The five names clients switch on: string, list, hash, set, zset.
    // Exact lowercase spelling is part of the protocol -- "sorted_set" breaks clients.
    let mut c = connect();
    c.del(&["ks_t_s", "ks_t_l", "ks_t_h", "ks_t_set", "ks_t_z"]);

    c.cmd(&["SET", "ks_t_s", "v"]);
    assert_eq!(c.cmd(&["TYPE", "ks_t_s"]), simple("string"));

    c.cmd(&["RPUSH", "ks_t_l", "v"]);
    assert_eq!(c.cmd(&["TYPE", "ks_t_l"]), simple("list"));

    c.cmd(&["HSET", "ks_t_h", "f", "v"]);
    assert_eq!(c.cmd(&["TYPE", "ks_t_h"]), simple("hash"));

    c.cmd(&["SADD", "ks_t_set", "v"]);
    assert_eq!(c.cmd(&["TYPE", "ks_t_set"]), simple("set"));

    c.cmd(&["ZADD", "ks_t_z", "1", "v"]);
    assert_eq!(c.cmd(&["TYPE", "ks_t_z"]), simple("zset"));
}

// ---------------------------------------------------------------------------
// KEYS
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 6: key commands (KEYS)"]
fn keys_with_pattern() {
    // `KEYS kp_foo*` returns only the three kp_foo_* keys, not the kp_key_* ones.
    let mut c = connect();
    for key in ["kp_key_x", "kp_key_y", "kp_key_z", "kp_foo_a", "kp_foo_b", "kp_foo_c"] {
        c.cmd(&["SET", key, "hello"]);
    }
    assert_eq!(
        c.cmd(&["KEYS", "kp_foo*"]).sorted(),
        vec!["kp_foo_a", "kp_foo_b", "kp_foo_c"]
    );
    c.del(&["kp_key_x", "kp_key_y", "kp_key_z", "kp_foo_a", "kp_foo_b", "kp_foo_c"]);
}

#[test]
#[ignore = "Session 6: key commands (KEYS glob matching)"]
fn keys_glob_metacharacters() {
    // Redis glob, not regex: `*` any run, `?` one char, `[abc]` / `[a-c]` /
    // `[^a]` classes, `\` escapes. Handing the pattern to a regex crate breaks
    // on any key containing `.` or `+`.
    let mut c = connect();
    for key in ["kg_a", "kg_b", "kg_c", "kg_aa", "kg_ab"] {
        c.cmd(&["SET", key, "v"]);
    }
    assert_eq!(c.cmd(&["KEYS", "kg_?"]).sorted(), vec!["kg_a", "kg_b", "kg_c"]);
    assert_eq!(c.cmd(&["KEYS", "kg_[ab]"]).sorted(), vec!["kg_a", "kg_b"]);
    assert_eq!(c.cmd(&["KEYS", "kg_[a-b]"]).sorted(), vec!["kg_a", "kg_b"]);
    assert_eq!(c.cmd(&["KEYS", "kg_a?"]).sorted(), vec!["kg_aa", "kg_ab"]);
    c.del(&["kg_a", "kg_b", "kg_c", "kg_aa", "kg_ab"]);
}

#[test]
#[ignore = "Session 6: key commands (KEYS)"]
fn keys_star_returns_everything() {
    // `KEYS *` lists every key, and DBSIZE agrees with how many that is.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for key in ["ka_1", "ka_2", "ka_3"] {
        c.cmd(&["SET", key, "v"]);
    }
    assert_eq!(c.cmd(&["KEYS", "*"]).sorted(), vec!["ka_1", "ka_2", "ka_3"]);
    assert_eq!(c.cmd(&["DBSIZE"]), int(3));
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 6: key commands (KEYS pattern matcher must not backtrack exponentially)"]
fn regression_for_pattern_matching_long_nested_loops() {
    // 20 `*`s against a 50,000-char key. A backtracking matcher takes forever
    // here; Redis's never backs up more than one `*`. Note this test hangs
    // rather than fails when you get it wrong.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    c.cmd(&["SET", &"a".repeat(50000), "v"]);
    assert_eq!(
        c.cmd(&["KEYS", "a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*a*b"]),
        arr(vec![])
    );
    c.cmd(&["FLUSHALL"]);
}

// ---------------------------------------------------------------------------
// RENAME
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 6: key commands (RENAME)"]
fn rename_basic_usage() {
    // The value moves to the new name and the old name is gone.
    let mut c = connect();
    c.cmd(&["SET", "ks_mykey", "hello"]);
    assert_eq!(c.cmd(&["RENAME", "ks_mykey", "ks_mykey1"]), ok());
    assert_eq!(c.cmd(&["RENAME", "ks_mykey1", "ks_mykey2"]), ok());
    assert_eq!(c.cmd(&["GET", "ks_mykey2"]), bulk("hello"));
    assert_eq!(c.cmd(&["EXISTS", "ks_mykey"]), int(0));
    c.del(&["ks_mykey2"]);
}

#[test]
#[ignore = "Session 6: key commands (RENAME)"]
fn rename_against_already_existing_key() {
    // RENAME clobbers the destination without complaint. RENAMENX refuses instead.
    let mut c = connect();
    c.cmd(&["SET", "ks_r1", "a"]);
    c.cmd(&["SET", "ks_r2", "b"]);
    assert_eq!(c.cmd(&["RENAME", "ks_r2", "ks_r1"]), ok());
    assert_eq!(c.cmd(&["GET", "ks_r1"]), bulk("b"));
    assert_eq!(c.cmd(&["EXISTS", "ks_r2"]), int(0));
    c.del(&["ks_r1"]);
}

#[test]
#[ignore = "Session 6: key commands (RENAME)"]
fn rename_against_non_existing_source_key() {
    // Renaming a key that is not there is an error, not a silent no-op.
    let mut c = connect();
    c.del(&["ks_nokey"]);
    assert_error(&c.cmd(&["RENAME", "ks_nokey", "ks_foobar"]), "ERR");
}

#[test]
#[ignore = "Session 6: key commands (RENAME)"]
fn rename_where_source_and_dest_are_the_same() {
    // `RENAME k k` keeps the value (and errors if k is missing). The trap:
    // deleting the destination before inserting the source wipes the key.
    let mut c = connect();
    c.cmd(&["SET", "ks_same", "foo"]);
    assert_eq!(c.cmd(&["RENAME", "ks_same", "ks_same"]), ok());
    assert_eq!(c.cmd(&["GET", "ks_same"]), bulk("foo"));

    c.del(&["ks_same"]);
    assert_error(&c.cmd(&["RENAME", "ks_same", "ks_same"]), "ERR");
}

#[test]
#[ignore = "Session 6: key commands (RENAMENX)"]
fn renamenx_basic_usage() {
    // Destination free, so the rename happens and returns 1.
    let mut c = connect();
    c.del(&["ks_nx1", "ks_nx2"]);
    c.cmd(&["SET", "ks_nx1", "foobar"]);
    assert_eq!(c.cmd(&["RENAMENX", "ks_nx1", "ks_nx2"]), int(1));
    assert_eq!(c.cmd(&["GET", "ks_nx2"]), bulk("foobar"));
    assert_eq!(c.cmd(&["EXISTS", "ks_nx1"]), int(0));
    c.del(&["ks_nx2"]);
}

#[test]
#[ignore = "Session 6: key commands (RENAMENX)"]
fn renamenx_against_already_existing_key() {
    // Destination taken, so RENAMENX returns 0 and both keys keep their values.
    let mut c = connect();
    c.cmd(&["SET", "ks_nx1", "foo"]);
    c.cmd(&["SET", "ks_nx2", "bar"]);
    assert_eq!(c.cmd(&["RENAMENX", "ks_nx1", "ks_nx2"]), int(0));
    // Nothing moved.
    assert_eq!(c.cmd(&["GET", "ks_nx1"]), bulk("foo"));
    assert_eq!(c.cmd(&["GET", "ks_nx2"]), bulk("bar"));
    c.del(&["ks_nx1", "ks_nx2"]);
}

#[test]
#[ignore = "Session 11: expiry (RENAME must carry the TTL)"]
fn rename_with_volatile_key_moves_the_ttl() {
    // RENAME carries the TTL along: ks_v1 has ~100s left, so ks_v2 does too.
    let mut c = connect();
    c.del(&["ks_v1", "ks_v2"]);
    c.cmd(&["SET", "ks_v1", "foo"]);
    c.cmd(&["EXPIRE", "ks_v1", "100"]);
    let ttl = c.cmd(&["TTL", "ks_v1"]).int();
    assert!(ttl > 95 && ttl <= 100, "ttl was {ttl}");
    c.cmd(&["RENAME", "ks_v1", "ks_v2"]);
    let ttl = c.cmd(&["TTL", "ks_v2"]).int();
    assert!(ttl > 95 && ttl <= 100, "ttl was {ttl}");
    c.del(&["ks_v2"]);
}

#[test]
#[ignore = "Session 11: expiry (RENAME must not inherit the target's TTL)"]
fn rename_with_volatile_key_does_not_inherit_ttl_of_target() {
    // ...and must NOT pick up the destination's old TTL. Overwriting only the
    // value leaves the target's TTL in place and quietly expires data that was
    // meant to stay.
    let mut c = connect();
    c.del(&["ks_v1", "ks_v2"]);
    c.cmd(&["SET", "ks_v1", "foo"]);
    c.cmd(&["SET", "ks_v2", "bar"]);
    c.cmd(&["EXPIRE", "ks_v2", "100"]);
    assert_eq!(c.cmd(&["TTL", "ks_v1"]), int(-1));
    assert!(c.cmd(&["TTL", "ks_v2"]).int() > 0);
    c.cmd(&["RENAME", "ks_v1", "ks_v2"]);
    assert_eq!(c.cmd(&["TTL", "ks_v2"]), int(-1));
    c.del(&["ks_v2"]);
}

// ---------------------------------------------------------------------------
// COPY
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: COPY"]
fn copy_basic_usage_for_string() {
    // COPY leaves the source in place -- unlike RENAME, both keys exist after.
    let mut c = connect();
    c.del(&["ks_c1", "ks_c2"]);
    c.cmd(&["SET", "ks_c1", "foobar"]);
    assert_eq!(c.cmd(&["COPY", "ks_c1", "ks_c2"]), int(1));
    assert_eq!(c.cmd(&["GET", "ks_c2"]), bulk("foobar"));
    assert_eq!(c.cmd(&["GET", "ks_c1"]), bulk("foobar"));
    c.del(&["ks_c1", "ks_c2"]);
}

#[test]
#[ignore = "bonus: COPY"]
fn copy_does_not_replace_without_replace_option() {
    // COPY onto an existing key returns 0 and changes nothing;
    // `COPY src dst REPLACE` returns 1 and overwrites.
    let mut c = connect();
    c.cmd(&["SET", "ks_c1", "foobar"]);
    c.cmd(&["SET", "ks_c2", "hello"]);
    assert_eq!(c.cmd(&["COPY", "ks_c1", "ks_c2"]), int(0));
    assert_eq!(c.cmd(&["GET", "ks_c2"]), bulk("hello"));
    assert_eq!(c.cmd(&["COPY", "ks_c1", "ks_c2", "REPLACE"]), int(1));
    assert_eq!(c.cmd(&["GET", "ks_c2"]), bulk("foobar"));
    c.del(&["ks_c1", "ks_c2"]);
}

#[test]
#[ignore = "bonus: COPY (copies must be independent)"]
fn copy_ensures_copied_data_is_independent() {
    // APPEND to the source must not touch the copy: "foo" -> "foobar" while the
    // copy stays "foo". `.clone()` gives you this for free; a later
    // `Arc<Vec<u8>>` sharing trick would quietly break it.
    let mut c = connect();
    c.del(&["ks_c1", "ks_c2"]);
    c.cmd(&["SET", "ks_c1", "foo"]);
    c.cmd(&["COPY", "ks_c1", "ks_c2"]);
    c.cmd(&["APPEND", "ks_c1", "bar"]);
    assert_eq!(c.cmd(&["GET", "ks_c1"]), bulk("foobar"));
    assert_eq!(c.cmd(&["GET", "ks_c2"]), bulk("foo"));
    c.del(&["ks_c1", "ks_c2"]);
}

#[test]
#[ignore = "bonus: COPY + Session 11: expiry"]
fn copy_copies_expire_metadata_as_well() {
    // The copy inherits the source's TTL (~100s here).
    let mut c = connect();
    c.del(&["ks_c1", "ks_c2"]);
    c.cmd(&["SET", "ks_c1", "foo", "EX", "100"]);
    c.cmd(&["COPY", "ks_c1", "ks_c2"]);
    let ttl = c.cmd(&["TTL", "ks_c2"]).int();
    assert!(ttl > 95 && ttl <= 100, "ttl was {ttl}");

    // ...and gets no TTL when the source has none.
    c.del(&["ks_c1", "ks_c2"]);
    c.cmd(&["SET", "ks_c1", "foo"]);
    c.cmd(&["COPY", "ks_c1", "ks_c2"]);
    assert_eq!(c.cmd(&["TTL", "ks_c2"]), int(-1));
    c.del(&["ks_c1", "ks_c2"]);
}

// ---------------------------------------------------------------------------
// DBSIZE / FLUSHALL
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 6: key commands (DBSIZE)"]
fn dbsize_and_flushall() {
    // DBSIZE tracks the key count: 0 -> 10 -> 0 after FLUSHALL.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    assert_eq!(c.cmd(&["DBSIZE"]), int(0));
    for i in 0..10 {
        c.cmd(&["SET", &format!("ks_d{i}"), "v"]);
    }
    assert_eq!(c.cmd(&["DBSIZE"]), int(10));
    assert_eq!(c.cmd(&["FLUSHALL"]), ok());
    assert_eq!(c.cmd(&["DBSIZE"]), int(0));
}

#[test]
#[ignore = "Session 11: expiry (DBSIZE must not count logically-expired keys)"]
fn dbsize_does_not_count_expired_keys() {
    // A key past its TTL is still in the HashMap until the sweep runs, but
    // DBSIZE must already report 0. Counting it leaks your internals to clients.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    c.cmd(&["SET", "ks_gone", "v", "PX", "50"]);
    assert_eq!(c.cmd(&["DBSIZE"]), int(1));
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert_eq!(c.cmd(&["DBSIZE"]), int(0));
}
