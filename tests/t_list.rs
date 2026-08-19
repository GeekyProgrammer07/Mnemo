//! Ported from `redis/tests/unit/type/list.tcl`.
//!
//! Covers every non-blocking list command: push, pop, range, index, trim,
//! insert, and moves between lists.
//!
//! Skipped: the listpack/quicklist encoding loops (a `VecDeque` has no encoding
//! to assert, and the behaviour is the same either way), and the blocking
//! commands `BLPOP`/`BRPOP`/`BLMOVE` (they need a wait queue -- a much bigger
//! feature, not on the roadmap).
//!
//! Use `VecDeque`: pushes and pops hit both ends, and it is O(1) at both.
//! `Vec` would be O(n) at the front because every element shifts.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// LPUSH / RPUSH / LRANGE / LLEN
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 7: list commands (RPUSH/LRANGE)"]
fn rpush_appends_to_the_tail() {
    // The reply is the length after the push: RPUSH k a => 1, RPUSH k b => 2.
    // So a client can push and check a size cap in one round trip.
    let mut c = connect();
    c.del(&["lst_r"]);
    assert_eq!(c.cmd(&["RPUSH", "lst_r", "a"]), int(1));
    assert_eq!(c.cmd(&["RPUSH", "lst_r", "b"]), int(2));
    assert_eq!(c.cmd(&["RPUSH", "lst_r", "c"]), int(3));
    assert_eq!(c.cmd(&["LRANGE", "lst_r", "0", "-1"]), bulks(&["a", "b", "c"]));
    c.del(&["lst_r"]);
}

#[test]
#[ignore = "Session 7: list commands (LPUSH/LRANGE)"]
fn lpush_prepends_to_the_head() {
    // LPUSH a, b, c one at a time => list reads c, b, a.
    let mut c = connect();
    c.del(&["lst_l"]);
    c.cmd(&["LPUSH", "lst_l", "a"]);
    c.cmd(&["LPUSH", "lst_l", "b"]);
    c.cmd(&["LPUSH", "lst_l", "c"]);
    assert_eq!(c.cmd(&["LRANGE", "lst_l", "0", "-1"]), bulks(&["c", "b", "a"]));
    c.del(&["lst_l"]);
}

#[test]
#[ignore = "Session 7: variadic LPUSH/RPUSH"]
fn variadic_push_applies_arguments_left_to_right() {
    // Arguments are pushed left to right, so LPUSH k a b c => c, b, a
    // (RPUSH k a b c => a, b, c). Easy to get backwards.
    let mut c = connect();
    c.del(&["lst_v"]);
    assert_eq!(c.cmd(&["RPUSH", "lst_v", "a", "b", "c"]), int(3));
    assert_eq!(c.cmd(&["LRANGE", "lst_v", "0", "-1"]), bulks(&["a", "b", "c"]));

    c.del(&["lst_v"]);
    assert_eq!(c.cmd(&["LPUSH", "lst_v", "a", "b", "c"]), int(3));
    assert_eq!(c.cmd(&["LRANGE", "lst_v", "0", "-1"]), bulks(&["c", "b", "a"]));
    c.del(&["lst_v"]);
}

#[test]
#[ignore = "Session 7: list commands (LPUSHX/RPUSHX)"]
fn pushx_only_pushes_to_an_existing_list() {
    // RPUSHX on a missing key returns 0 and creates nothing.
    let mut c = connect();
    c.del(&["lst_x"]);
    assert_eq!(c.cmd(&["RPUSHX", "lst_x", "a"]), int(0));
    assert_eq!(c.cmd(&["EXISTS", "lst_x"]), int(0), "must not create the key");
    c.cmd(&["RPUSH", "lst_x", "a"]);
    assert_eq!(c.cmd(&["RPUSHX", "lst_x", "b"]), int(2));
    assert_eq!(c.cmd(&["LPUSHX", "lst_x", "z"]), int(3));
    assert_eq!(c.cmd(&["LRANGE", "lst_x", "0", "-1"]), bulks(&["z", "a", "b"]));
    c.del(&["lst_x"]);
}

#[test]
#[ignore = "Session 7: list commands (LLEN)"]
fn llen_against_existing_and_missing_keys() {
    // A missing key counts as length 0, not an error.
    let mut c = connect();
    c.del(&["lst_n"]);
    assert_eq!(c.cmd(&["LLEN", "lst_n"]), int(0));
    c.cmd(&["RPUSH", "lst_n", "a", "b", "c"]);
    assert_eq!(c.cmd(&["LLEN", "lst_n"]), int(3));
    c.del(&["lst_n"]);
}

#[test]
#[ignore = "Session 7: list commands (LRANGE)"]
fn lrange_index_semantics() {
    // Both ends included, negatives count from the tail: on 0..9,
    // LRANGE k 1 3 => 1,2,3 and LRANGE k -3 -1 => 7,8,9.
    // Out-of-range clamps to an empty array, never an error.
    let mut c = connect();
    c.del(&["lst_rg"]);
    c.cmd(&["RPUSH", "lst_rg", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);
    assert_eq!(c.cmd(&["LRANGE", "lst_rg", "1", "3"]), bulks(&["1", "2", "3"]));
    assert_eq!(c.cmd(&["LRANGE", "lst_rg", "-3", "-1"]), bulks(&["7", "8", "9"]));
    assert_eq!(c.cmd(&["LRANGE", "lst_rg", "0", "0"]), bulks(&["0"]));
    assert_eq!(c.cmd(&["LRANGE", "lst_rg", "-100", "100"]).array().len(), 10);
    assert_eq!(c.cmd(&["LRANGE", "lst_rg", "5", "3"]), arr(vec![]));
    assert_eq!(c.cmd(&["LRANGE", "lst_rg", "100", "200"]), arr(vec![]));
    c.del(&["lst_rg"]);
}

#[test]
#[ignore = "Session 7: list commands (LRANGE against a missing key)"]
fn lrange_against_non_existing_key() {
    // LRANGE on a missing key is an empty array, not nil and not an error.
    // A missing collection acts like an empty one, except for EXISTS/TYPE.
    let mut c = connect();
    c.del(&["lst_missing"]);
    assert_eq!(c.cmd(&["LRANGE", "lst_missing", "0", "-1"]), arr(vec![]));
}

// ---------------------------------------------------------------------------
// LPOP / RPOP
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 7: list commands (LPOP/RPOP)"]
fn lpop_and_rpop_take_from_each_end() {
    // On a,b,c,d: LPOP => "a" (head), RPOP => "d" (tail).
    let mut c = connect();
    c.del(&["lst_p"]);
    c.cmd(&["RPUSH", "lst_p", "a", "b", "c", "d"]);
    assert_eq!(c.cmd(&["LPOP", "lst_p"]), bulk("a"));
    assert_eq!(c.cmd(&["RPOP", "lst_p"]), bulk("d"));
    assert_eq!(c.cmd(&["LRANGE", "lst_p", "0", "-1"]), bulks(&["b", "c"]));
    c.del(&["lst_p"]);
}

#[test]
#[ignore = "Session 7: list commands (POP against a missing key)"]
fn lpop_rpop_against_non_existing_key() {
    // Nothing to pop => nil, not an error and not an empty string.
    let mut c = connect();
    c.del(&["lst_missing"]);
    assert!(c.cmd(&["LPOP", "lst_missing"]).is_nil());
    assert!(c.cmd(&["RPOP", "lst_missing"]).is_nil());
}

#[test]
#[ignore = "Session 7: an emptied list must delete its key"]
fn popping_the_last_element_deletes_the_key() {
    // Redis has no empty collections. RPUSH k a, LPOP k => EXISTS k is 0.
    // Leave an empty VecDeque behind and EXISTS says 1 for a gone key.
    // Every collection type needs this, so it wants one shared helper.
    let mut c = connect();
    c.del(&["lst_e"]);
    c.cmd(&["RPUSH", "lst_e", "a"]);
    assert_eq!(c.cmd(&["LPOP", "lst_e"]), bulk("a"));
    assert_eq!(c.cmd(&["EXISTS", "lst_e"]), int(0));
    assert_eq!(c.cmd(&["TYPE", "lst_e"]), simple("none"));
}

#[test]
#[ignore = "bonus: LPOP/RPOP with a count"]
fn lpop_rpop_with_count() {
    // With a count the reply is an array, not a bulk string: LPOP k 2 => [a, b].
    // A missing key then gives a null array (*-1) instead of a null bulk ($-1),
    // so the same command has two nil shapes depending on argument count.
    let mut c = connect();
    c.del(&["lst_ct"]);
    c.cmd(&["RPUSH", "lst_ct", "a", "b", "c", "d", "e"]);
    assert_eq!(c.cmd(&["LPOP", "lst_ct", "2"]), bulks(&["a", "b"]));
    assert_eq!(c.cmd(&["RPOP", "lst_ct", "2"]), bulks(&["e", "d"]));
    // A count larger than the list returns everything, not an error.
    assert_eq!(c.cmd(&["LPOP", "lst_ct", "10"]), bulks(&["c"]));
    assert_eq!(c.cmd(&["EXISTS", "lst_ct"]), int(0));

    c.del(&["lst_ct"]);
    assert_eq!(c.cmd(&["LPOP", "lst_ct", "2"]), Reply::NilArray);
    // Count 0 on an existing key is an empty array.
    c.cmd(&["RPUSH", "lst_ct", "a"]);
    assert_eq!(c.cmd(&["LPOP", "lst_ct", "0"]), arr(vec![]));
    assert_error(&c.cmd(&["LPOP", "lst_ct", "-1"]), "ERR value is out of range");
    c.del(&["lst_ct"]);
}

// ---------------------------------------------------------------------------
// LINDEX / LSET
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: LINDEX"]
fn lindex_against_various_positions() {
    // Index 0 is the head, -1 the tail. On a,b,c: LINDEX k -1 => "c".
    let mut c = connect();
    c.del(&["lst_i"]);
    c.cmd(&["RPUSH", "lst_i", "a", "b", "c"]);
    assert_eq!(c.cmd(&["LINDEX", "lst_i", "0"]), bulk("a"));
    assert_eq!(c.cmd(&["LINDEX", "lst_i", "2"]), bulk("c"));
    assert_eq!(c.cmd(&["LINDEX", "lst_i", "-1"]), bulk("c"));
    assert_eq!(c.cmd(&["LINDEX", "lst_i", "-3"]), bulk("a"));
    // Out of range is nil, not an error.
    assert!(c.cmd(&["LINDEX", "lst_i", "100"]).is_nil());
    assert!(c.cmd(&["LINDEX", "lst_i", "-100"]).is_nil());
    c.del(&["lst_i"]);
}

#[test]
#[ignore = "bonus: LSET"]
fn lset_replaces_an_element() {
    // LSET overwrites in place; it never grows the list.
    let mut c = connect();
    c.del(&["lst_s"]);
    c.cmd(&["RPUSH", "lst_s", "a", "b", "c"]);
    assert_eq!(c.cmd(&["LSET", "lst_s", "1", "B"]), ok());
    assert_eq!(c.cmd(&["LSET", "lst_s", "-1", "C"]), ok());
    assert_eq!(c.cmd(&["LRANGE", "lst_s", "0", "-1"]), bulks(&["a", "B", "C"]));
    // Out of range is an error here, unlike LINDEX which returns nil.
    assert_error(&c.cmd(&["LSET", "lst_s", "10", "x"]), "ERR index out of range");
    c.del(&["lst_s"]);
    assert_error(&c.cmd(&["LSET", "lst_s", "0", "x"]), "ERR no such key");
}

// ---------------------------------------------------------------------------
// LREM / LTRIM / LINSERT
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: LREM"]
fn lrem_removes_by_count_and_direction() {
    // The sign of count picks the direction. On a,b,a,c,a:
    // LREM k 2 a => b,c,a; LREM k -2 a => a,b,c; LREM k 0 a => b,c.
    let mut c = connect();

    c.del(&["lst_rm"]);
    c.cmd(&["RPUSH", "lst_rm", "a", "b", "a", "c", "a"]);
    assert_eq!(c.cmd(&["LREM", "lst_rm", "2", "a"]), int(2));
    assert_eq!(c.cmd(&["LRANGE", "lst_rm", "0", "-1"]), bulks(&["b", "c", "a"]));

    c.del(&["lst_rm"]);
    c.cmd(&["RPUSH", "lst_rm", "a", "b", "a", "c", "a"]);
    assert_eq!(c.cmd(&["LREM", "lst_rm", "-2", "a"]), int(2));
    assert_eq!(c.cmd(&["LRANGE", "lst_rm", "0", "-1"]), bulks(&["a", "b", "c"]));

    c.del(&["lst_rm"]);
    c.cmd(&["RPUSH", "lst_rm", "a", "b", "a", "c", "a"]);
    assert_eq!(c.cmd(&["LREM", "lst_rm", "0", "a"]), int(3));
    assert_eq!(c.cmd(&["LRANGE", "lst_rm", "0", "-1"]), bulks(&["b", "c"]));

    assert_eq!(c.cmd(&["LREM", "lst_rm", "0", "nosuchelement"]), int(0));
    c.del(&["lst_rm"]);
}

#[test]
#[ignore = "bonus: LREM emptying the list deletes the key"]
fn lrem_that_empties_the_list_deletes_the_key() {
    // The no-empty-collections rule applies to LREM too, not just LPOP.
    let mut c = connect();
    c.del(&["lst_rm"]);
    c.cmd(&["RPUSH", "lst_rm", "a", "a"]);
    assert_eq!(c.cmd(&["LREM", "lst_rm", "0", "a"]), int(2));
    assert_eq!(c.cmd(&["EXISTS", "lst_rm"]), int(0));
}

#[test]
#[ignore = "bonus: LTRIM"]
fn ltrim_keeps_only_the_given_range() {
    // LTRIM keeps the range and throws the rest away: on a..e, LTRIM k 1 3 => b,c,d.
    // This is how you cap a log: RPUSH, then LTRIM k -100 -1 keeps the newest 100.
    let mut c = connect();
    c.del(&["lst_t"]);
    c.cmd(&["RPUSH", "lst_t", "a", "b", "c", "d", "e"]);
    assert_eq!(c.cmd(&["LTRIM", "lst_t", "1", "3"]), ok());
    assert_eq!(c.cmd(&["LRANGE", "lst_t", "0", "-1"]), bulks(&["b", "c", "d"]));

    c.del(&["lst_t"]);
    c.cmd(&["RPUSH", "lst_t", "a", "b", "c", "d", "e"]);
    assert_eq!(c.cmd(&["LTRIM", "lst_t", "-2", "-1"]), ok());
    assert_eq!(c.cmd(&["LRANGE", "lst_t", "0", "-1"]), bulks(&["d", "e"]));

    // A range that picks nothing empties the list, so the key goes away.
    c.del(&["lst_t"]);
    c.cmd(&["RPUSH", "lst_t", "a", "b", "c"]);
    assert_eq!(c.cmd(&["LTRIM", "lst_t", "5", "10"]), ok());
    assert_eq!(c.cmd(&["EXISTS", "lst_t"]), int(0));
}

#[test]
#[ignore = "bonus: LINSERT"]
fn linsert_before_and_after_a_pivot() {
    // Insert relative to the first match of the pivot value, not an index:
    // on a,b,c, LINSERT k BEFORE b X => a,X,b,c.
    let mut c = connect();
    c.del(&["lst_in"]);
    c.cmd(&["RPUSH", "lst_in", "a", "b", "c"]);
    assert_eq!(c.cmd(&["LINSERT", "lst_in", "BEFORE", "b", "X"]), int(4));
    assert_eq!(c.cmd(&["LRANGE", "lst_in", "0", "-1"]), bulks(&["a", "X", "b", "c"]));
    assert_eq!(c.cmd(&["LINSERT", "lst_in", "AFTER", "b", "Y"]), int(5));
    assert_eq!(
        c.cmd(&["LRANGE", "lst_in", "0", "-1"]),
        bulks(&["a", "X", "b", "Y", "c"])
    );
    // -1 means "pivot not found"; 0 means "key not found". Two different replies.
    assert_eq!(c.cmd(&["LINSERT", "lst_in", "BEFORE", "nopivot", "z"]), int(-1));
    c.del(&["lst_in"]);
    assert_eq!(c.cmd(&["LINSERT", "lst_in", "BEFORE", "a", "z"]), int(0));
    assert_error(&c.cmd(&["LINSERT", "lst_in", "SIDEWAYS", "a", "z"]), "ERR syntax error");
}

// ---------------------------------------------------------------------------
// RPOPLPUSH / LMOVE
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: RPOPLPUSH"]
fn rpoplpush_moves_an_element_between_lists() {
    // Pops the tail of src and pushes it to the head of dst in one step:
    // src a,b,c => returns "c", src a,b and dst c.
    // One step matters: a crash between the pop and the push would lose the job.
    let mut c = connect();
    c.del(&["lst_src", "lst_dst"]);
    c.cmd(&["RPUSH", "lst_src", "a", "b", "c"]);
    assert_eq!(c.cmd(&["RPOPLPUSH", "lst_src", "lst_dst"]), bulk("c"));
    assert_eq!(c.cmd(&["LRANGE", "lst_src", "0", "-1"]), bulks(&["a", "b"]));
    assert_eq!(c.cmd(&["LRANGE", "lst_dst", "0", "-1"]), bulks(&["c"]));

    c.del(&["lst_empty"]);
    assert!(c.cmd(&["RPOPLPUSH", "lst_empty", "lst_dst"]).is_nil());
    assert_eq!(c.cmd(&["EXISTS", "lst_dst"]), int(1), "dst untouched");
    c.del(&["lst_src", "lst_dst"]);
}

#[test]
#[ignore = "bonus: RPOPLPUSH with the same source and destination"]
fn rpoplpush_with_the_same_key_rotates_the_list() {
    // Same key on both sides is legal and rotates: a,b,c => c,a,b.
    // Pop-then-look-the-key-up-again breaks on a one-element list: the pop
    // deletes the key, and the push finds nothing.
    let mut c = connect();
    c.del(&["lst_rot"]);
    c.cmd(&["RPUSH", "lst_rot", "a", "b", "c"]);
    assert_eq!(c.cmd(&["RPOPLPUSH", "lst_rot", "lst_rot"]), bulk("c"));
    assert_eq!(c.cmd(&["LRANGE", "lst_rot", "0", "-1"]), bulks(&["c", "a", "b"]));

    c.del(&["lst_one"]);
    c.cmd(&["RPUSH", "lst_one", "a"]);
    assert_eq!(c.cmd(&["RPOPLPUSH", "lst_one", "lst_one"]), bulk("a"));
    assert_eq!(c.cmd(&["LRANGE", "lst_one", "0", "-1"]), bulks(&["a"]));
    c.del(&["lst_rot", "lst_one"]);
}

#[test]
#[ignore = "bonus: LMOVE"]
fn lmove_with_explicit_directions() {
    // LMOVE is RPOPLPUSH with the ends spelled out: LEFT RIGHT takes the head of
    // src and appends it to dst. Anything but LEFT/RIGHT is a syntax error.
    let mut c = connect();
    c.del(&["lst_src", "lst_dst"]);
    c.cmd(&["RPUSH", "lst_src", "a", "b", "c"]);
    assert_eq!(c.cmd(&["LMOVE", "lst_src", "lst_dst", "LEFT", "RIGHT"]), bulk("a"));
    assert_eq!(c.cmd(&["LRANGE", "lst_src", "0", "-1"]), bulks(&["b", "c"]));
    assert_eq!(c.cmd(&["LRANGE", "lst_dst", "0", "-1"]), bulks(&["a"]));
    assert_error(
        &c.cmd(&["LMOVE", "lst_src", "lst_dst", "UP", "DOWN"]),
        "ERR syntax error",
    );
    c.del(&["lst_src", "lst_dst"]);
}

// ---------------------------------------------------------------------------
// Type errors
//
// Rule: a list command on a key holding another type replies WRONGTYPE and
// changes nothing. SET k v then LPUSH k a must fail, not overwrite. Check the
// type before touching the value, on every key the command names.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 7: list commands type checking"]
fn list_commands_against_a_string_key() {
    let mut c = connect();
    c.del(&["lst_str"]);
    c.cmd(&["SET", "lst_str", "v"]);
    assert_wrongtype(&c.cmd(&["RPUSH", "lst_str", "a"]));
    assert_wrongtype(&c.cmd(&["LPUSH", "lst_str", "a"]));
    assert_wrongtype(&c.cmd(&["LPOP", "lst_str"]));
    assert_wrongtype(&c.cmd(&["RPOP", "lst_str"]));
    assert_wrongtype(&c.cmd(&["LLEN", "lst_str"]));
    assert_wrongtype(&c.cmd(&["LRANGE", "lst_str", "0", "-1"]));
    assert_wrongtype(&c.cmd(&["LINDEX", "lst_str", "0"]));
    assert_wrongtype(&c.cmd(&["LSET", "lst_str", "0", "x"]));
    assert_wrongtype(&c.cmd(&["LREM", "lst_str", "0", "x"]));
    assert_wrongtype(&c.cmd(&["LTRIM", "lst_str", "0", "-1"]));
    c.del(&["lst_str"]);
}

#[test]
#[ignore = "Session 7: RPOPLPUSH type checking"]
fn rpoplpush_against_a_wrong_type_destination() {
    // Check dst before popping src. Pop first and the element is gone when the
    // push fails.
    let mut c = connect();
    c.del(&["lst_src", "lst_dst"]);
    c.cmd(&["RPUSH", "lst_src", "a", "b"]);
    c.cmd(&["SET", "lst_dst", "v"]);
    assert_wrongtype(&c.cmd(&["RPOPLPUSH", "lst_src", "lst_dst"]));
    assert_eq!(
        c.cmd(&["LRANGE", "lst_src", "0", "-1"]),
        bulks(&["a", "b"]),
        "source must be untouched when the destination rejects the push"
    );
    c.del(&["lst_src", "lst_dst"]);
}

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 7: list commands under load"]
fn mass_push_and_pop_preserves_order() {
    // Push 0..999, pop them back in the same order. Catches off-by-one bugs a
    // three-element list hides, and any accidental O(n^2).
    let mut c = connect();
    c.del(&["lst_mass"]);
    for i in 0..1000 {
        c.cmd(&["RPUSH", "lst_mass", &i.to_string()]);
    }
    assert_eq!(c.cmd(&["LLEN", "lst_mass"]), int(1000));
    assert_eq!(c.cmd(&["LRANGE", "lst_mass", "0", "-1"]).array().len(), 1000);
    assert_eq!(c.cmd(&["LINDEX", "lst_mass", "500"]), bulk("500"));
    for i in 0..1000 {
        assert_eq!(c.cmd(&["LPOP", "lst_mass"]), bulk(&i.to_string()));
    }
    assert_eq!(c.cmd(&["EXISTS", "lst_mass"]), int(0));
}
