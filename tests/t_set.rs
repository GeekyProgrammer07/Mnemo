//! Ported from `redis/tests/unit/type/set.tcl`.
//!
//! Covers add/remove/membership, the union-intersect-diff family and their
//! STORE forms, and the random picks SPOP / SRANDMEMBER.
//!
//! Skipped: the intset/listpack/hashtable encoding checks. Those are a memory
//! trick with no visible behaviour difference; `HashSet<Vec<u8>>` behaves the same.
//!
//! Use `HashSet<Vec<u8>>`: SADD and SISMEMBER are O(1), and the set operations
//! want O(1) membership too. Sets have no order, so most asserts below sort first.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// SADD / SREM / SCARD / SISMEMBER
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 9: set commands (SADD/SMEMBERS)"]
fn sadd_and_smembers_basic() {
    // SADD counts only new members: SADD s a => 1, SADD s a again => 0.
    // That 0/1 is a one-round-trip "have I seen this before?".
    let mut c = connect();
    c.del(&["set_s"]);
    assert_eq!(c.cmd(&["SADD", "set_s", "a"]), int(1));
    assert_eq!(c.cmd(&["SADD", "set_s", "a"]), int(0));
    assert_eq!(c.cmd(&["SADD", "set_s", "b", "c"]), int(2));
    assert_eq!(c.cmd(&["SMEMBERS", "set_s"]).sorted(), vec!["a", "b", "c"]);
    c.del(&["set_s"]);
}

#[test]
#[ignore = "Session 9: variadic SADD with duplicates"]
fn sadd_with_duplicate_arguments_counts_once() {
    // Duplicates inside one call collapse: SADD s a a a => 1, SCARD 1.
    let mut c = connect();
    c.del(&["set_d"]);
    assert_eq!(c.cmd(&["SADD", "set_d", "a", "a", "a"]), int(1));
    assert_eq!(c.cmd(&["SCARD", "set_d"]), int(1));
    c.del(&["set_d"]);
}

#[test]
#[ignore = "Session 9: set commands (SCARD)"]
fn scard_against_existing_and_missing_keys() {
    // A missing key counts as 0, not an error.
    let mut c = connect();
    c.del(&["set_c"]);
    assert_eq!(c.cmd(&["SCARD", "set_c"]), int(0));
    c.cmd(&["SADD", "set_c", "a", "b", "c"]);
    assert_eq!(c.cmd(&["SCARD", "set_c"]), int(3));
    c.del(&["set_c"]);
}

#[test]
#[ignore = "Session 9: set commands (SISMEMBER)"]
fn sismember_against_existing_and_missing_members() {
    // 1 or 0, never nil -- and a missing key answers 0 like an empty set.
    let mut c = connect();
    c.del(&["set_m"]);
    c.cmd(&["SADD", "set_m", "a"]);
    assert_eq!(c.cmd(&["SISMEMBER", "set_m", "a"]), int(1));
    assert_eq!(c.cmd(&["SISMEMBER", "set_m", "b"]), int(0));
    c.del(&["set_missing"]);
    assert_eq!(c.cmd(&["SISMEMBER", "set_missing", "a"]), int(0));
    c.del(&["set_m"]);
}

#[test]
#[ignore = "bonus: SMISMEMBER"]
fn smismember_checks_several_members_at_once() {
    // One 0/1 per member asked about, in the order asked: {a,b} queried
    // a, x, b => [1, 0, 1].
    let mut c = connect();
    c.del(&["set_mm"]);
    c.cmd(&["SADD", "set_mm", "a", "b"]);
    assert_eq!(
        c.cmd(&["SMISMEMBER", "set_mm", "a", "x", "b"]),
        arr(vec![int(1), int(0), int(1)])
    );
    c.del(&["set_mm"]);
}

#[test]
#[ignore = "Session 9: set commands (SREM)"]
fn srem_removes_members_and_counts_them() {
    // Counts only members that were there: SREM s b c missing => 2.
    let mut c = connect();
    c.del(&["set_r"]);
    c.cmd(&["SADD", "set_r", "a", "b", "c", "d"]);
    assert_eq!(c.cmd(&["SREM", "set_r", "a"]), int(1));
    assert_eq!(c.cmd(&["SREM", "set_r", "nosuchmember"]), int(0));
    assert_eq!(c.cmd(&["SREM", "set_r", "b", "c", "nosuchmember"]), int(2));
    assert_eq!(c.cmd(&["SMEMBERS", "set_r"]), bulks(&["d"]));
    c.del(&["set_r"]);
}

#[test]
#[ignore = "Session 9: removing the last member deletes the key"]
fn srem_of_the_last_member_deletes_the_key() {
    // No empty collections: SADD s a, SREM s a => EXISTS s is 0, TYPE none.
    let mut c = connect();
    c.del(&["set_e"]);
    c.cmd(&["SADD", "set_e", "a"]);
    assert_eq!(c.cmd(&["SREM", "set_e", "a"]), int(1));
    assert_eq!(c.cmd(&["EXISTS", "set_e"]), int(0));
    assert_eq!(c.cmd(&["TYPE", "set_e"]), simple("none"));
}

#[test]
#[ignore = "Session 9: set commands (SMEMBERS against a missing key)"]
fn smembers_against_non_existing_key() {
    // Empty array, not nil and not an error.
    let mut c = connect();
    c.del(&["set_missing"]);
    assert_eq!(c.cmd(&["SMEMBERS", "set_missing"]), arr(vec![]));
}

// ---------------------------------------------------------------------------
// SUNION / SINTER / SDIFF
//
// All three take any number of keys, treat a missing key as the empty set, and
// return members in no particular order -- so the asserts sort first.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 9: set commands (SUNION)"]
fn sunion_of_several_sets() {
    // Everything in any of the sets, each member once.
    let mut c = connect();
    c.del(&["set_1", "set_2", "set_3"]);
    c.cmd(&["SADD", "set_1", "a", "b", "c"]);
    c.cmd(&["SADD", "set_2", "b", "c", "d"]);
    c.cmd(&["SADD", "set_3", "e"]);
    assert_eq!(
        c.cmd(&["SUNION", "set_1", "set_2", "set_3"]).sorted(),
        vec!["a", "b", "c", "d", "e"]
    );
    c.del(&["set_1", "set_2", "set_3"]);
}

#[test]
#[ignore = "Session 9: set commands (SINTER)"]
fn sinter_of_several_sets() {
    // Only members in every set: {a,b,c,d}, {b,c,d,e}, {c,d} => c, d.
    let mut c = connect();
    c.del(&["set_1", "set_2", "set_3"]);
    c.cmd(&["SADD", "set_1", "a", "b", "c", "d"]);
    c.cmd(&["SADD", "set_2", "b", "c", "d", "e"]);
    c.cmd(&["SADD", "set_3", "c", "d"]);
    assert_eq!(
        c.cmd(&["SINTER", "set_1", "set_2", "set_3"]).sorted(),
        vec!["c", "d"]
    );
    c.del(&["set_1", "set_2", "set_3"]);
}

#[test]
#[ignore = "Session 9: SINTER short-circuits on a missing key"]
fn sinter_with_a_non_existing_key_is_empty() {
    // One missing key makes the whole intersection empty, whatever the others
    // hold. Bail out early instead of scanning a million-member set for nothing.
    let mut c = connect();
    c.del(&["set_1", "set_missing"]);
    c.cmd(&["SADD", "set_1", "a", "b", "c"]);
    assert_eq!(c.cmd(&["SINTER", "set_1", "set_missing"]), arr(vec![]));
    c.del(&["set_1"]);
}

#[test]
#[ignore = "Session 9: set commands (SDIFF)"]
fn sdiff_subtracts_later_sets_from_the_first() {
    // First set minus all the rest, so argument order matters:
    // SDIFF {a,b,c,d} {b} {c} => a, d, but SDIFF {b} {a,b,c,d} => empty.
    let mut c = connect();
    c.del(&["set_1", "set_2", "set_3"]);
    c.cmd(&["SADD", "set_1", "a", "b", "c", "d"]);
    c.cmd(&["SADD", "set_2", "b"]);
    c.cmd(&["SADD", "set_3", "c"]);
    assert_eq!(
        c.cmd(&["SDIFF", "set_1", "set_2", "set_3"]).sorted(),
        vec!["a", "d"]
    );
    // Reversed arguments, different answer.
    assert_eq!(c.cmd(&["SDIFF", "set_2", "set_1"]), arr(vec![]));
    c.del(&["set_1", "set_2", "set_3"]);
}

#[test]
#[ignore = "Session 9: set operations with a single argument"]
fn set_operations_with_one_key_return_that_set() {
    // One key is legal: all three just return that set's members.
    let mut c = connect();
    c.del(&["set_1"]);
    c.cmd(&["SADD", "set_1", "a", "b"]);
    assert_eq!(c.cmd(&["SUNION", "set_1"]).sorted(), vec!["a", "b"]);
    assert_eq!(c.cmd(&["SINTER", "set_1"]).sorted(), vec!["a", "b"]);
    assert_eq!(c.cmd(&["SDIFF", "set_1"]).sorted(), vec!["a", "b"]);
    c.del(&["set_1"]);
}

#[test]
#[ignore = "bonus: SUNIONSTORE / SINTERSTORE / SDIFFSTORE"]
fn store_variants_write_the_result_to_a_destination() {
    // The STORE forms reply with the size of the result, not the members, and
    // overwrite the destination whatever type it held.
    let mut c = connect();
    c.del(&["set_1", "set_2", "set_dst"]);
    c.cmd(&["SADD", "set_1", "a", "b", "c"]);
    c.cmd(&["SADD", "set_2", "b", "c", "d"]);

    assert_eq!(c.cmd(&["SUNIONSTORE", "set_dst", "set_1", "set_2"]), int(4));
    assert_eq!(c.cmd(&["SMEMBERS", "set_dst"]).sorted(), vec!["a", "b", "c", "d"]);
    assert_eq!(c.cmd(&["SINTERSTORE", "set_dst", "set_1", "set_2"]), int(2));
    assert_eq!(c.cmd(&["SMEMBERS", "set_dst"]).sorted(), vec!["b", "c"]);
    assert_eq!(c.cmd(&["SDIFFSTORE", "set_dst", "set_1", "set_2"]), int(1));
    assert_eq!(c.cmd(&["SMEMBERS", "set_dst"]), bulks(&["a"]));
    c.del(&["set_1", "set_2", "set_dst"]);
}

#[test]
#[ignore = "bonus: an empty STORE result deletes the destination"]
fn store_with_an_empty_result_deletes_the_destination() {
    // No empty collections, in its sneakiest form: an empty result deletes the
    // destination key -- even one that already held a string.
    let mut c = connect();
    c.del(&["set_1", "set_2", "set_dst"]);
    c.cmd(&["SADD", "set_1", "a"]);
    c.cmd(&["SADD", "set_2", "b"]);
    c.cmd(&["SET", "set_dst", "preexisting"]);
    assert_eq!(c.cmd(&["SINTERSTORE", "set_dst", "set_1", "set_2"]), int(0));
    assert_eq!(c.cmd(&["EXISTS", "set_dst"]), int(0));
    c.del(&["set_1", "set_2"]);
}

// ---------------------------------------------------------------------------
// SPOP / SRANDMEMBER / SMOVE
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: SPOP"]
fn spop_removes_and_returns_a_random_member() {
    // SPOP returns some member and takes it out; a missing key gives nil.
    let mut c = connect();
    c.del(&["set_p"]);
    c.cmd(&["SADD", "set_p", "a", "b", "c"]);
    let popped = c.cmd(&["SPOP", "set_p"]).str().to_string();
    assert!(["a", "b", "c"].contains(&popped.as_str()));
    assert_eq!(c.cmd(&["SCARD", "set_p"]), int(2));
    assert_eq!(c.cmd(&["SISMEMBER", "set_p", &popped]), int(0));

    c.del(&["set_missing"]);
    assert!(c.cmd(&["SPOP", "set_missing"]).is_nil());
    c.del(&["set_p"]);
}

#[test]
#[ignore = "bonus: SPOP with a count"]
fn spop_with_count() {
    // With a count the reply is an array, and a missing key gives an empty array
    // rather than the nil the no-count form returns.
    let mut c = connect();
    c.del(&["set_p"]);
    c.cmd(&["SADD", "set_p", "a", "b", "c", "d", "e"]);
    assert_eq!(c.cmd(&["SPOP", "set_p", "2"]).array().len(), 2);
    assert_eq!(c.cmd(&["SCARD", "set_p"]), int(3));
    // A count bigger than the set returns everything and deletes the key.
    assert_eq!(c.cmd(&["SPOP", "set_p", "100"]).array().len(), 3);
    assert_eq!(c.cmd(&["EXISTS", "set_p"]), int(0));

    c.del(&["set_missing"]);
    assert_eq!(c.cmd(&["SPOP", "set_missing", "2"]), arr(vec![]));
    assert_error(&c.cmd(&["SPOP", "set_missing", "-1"]), "ERR value is out of range");
}

#[test]
#[ignore = "bonus: SRANDMEMBER"]
fn srandmember_does_not_remove_anything() {
    // Reads without removing. Sign of the count changes the rule: on a 3-member
    // set, count 10 => 3 distinct members, count -10 => exactly 10 with repeats.
    let mut c = connect();
    c.del(&["set_rm"]);
    c.cmd(&["SADD", "set_rm", "a", "b", "c"]);
    assert!(["a", "b", "c"].contains(&c.cmd(&["SRANDMEMBER", "set_rm"]).str()));
    assert_eq!(c.cmd(&["SCARD", "set_rm"]), int(3), "must not remove");

    assert_eq!(c.cmd(&["SRANDMEMBER", "set_rm", "2"]).array().len(), 2);
    assert_eq!(c.cmd(&["SRANDMEMBER", "set_rm", "10"]).array().len(), 3);
    assert_eq!(c.cmd(&["SRANDMEMBER", "set_rm", "-10"]).array().len(), 10);
    assert_eq!(c.cmd(&["SRANDMEMBER", "set_rm", "0"]), arr(vec![]));

    c.del(&["set_missing"]);
    assert!(c.cmd(&["SRANDMEMBER", "set_missing"]).is_nil());
    assert_eq!(c.cmd(&["SRANDMEMBER", "set_missing", "3"]), arr(vec![]));
    c.del(&["set_rm"]);
}

#[test]
#[ignore = "bonus: SMOVE"]
fn smove_between_sets() {
    // Moves one named member from src to dst in a single step, replying 1 or 0.
    let mut c = connect();
    c.del(&["set_src", "set_dst"]);
    c.cmd(&["SADD", "set_src", "a", "b"]);
    c.cmd(&["SADD", "set_dst", "c"]);
    assert_eq!(c.cmd(&["SMOVE", "set_src", "set_dst", "a"]), int(1));
    assert_eq!(c.cmd(&["SMEMBERS", "set_src"]), bulks(&["b"]));
    assert_eq!(c.cmd(&["SMEMBERS", "set_dst"]).sorted(), vec!["a", "c"]);
    // A member that is not there: nothing happens, reply 0.
    assert_eq!(c.cmd(&["SMOVE", "set_src", "set_dst", "nosuchmember"]), int(0));
    // Moving the last member deletes the source key.
    assert_eq!(c.cmd(&["SMOVE", "set_src", "set_dst", "b"]), int(1));
    assert_eq!(c.cmd(&["EXISTS", "set_src"]), int(0));
    c.del(&["set_dst"]);
}

// ---------------------------------------------------------------------------
// Type errors and volume
//
// Rule: a set command on a key holding another type replies WRONGTYPE and
// changes nothing. SET k v then SADD k a must fail, not overwrite.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 9: set commands type checking"]
fn set_commands_against_a_string_key() {
    let mut c = connect();
    c.del(&["set_str"]);
    c.cmd(&["SET", "set_str", "v"]);
    assert_wrongtype(&c.cmd(&["SADD", "set_str", "a"]));
    assert_wrongtype(&c.cmd(&["SREM", "set_str", "a"]));
    assert_wrongtype(&c.cmd(&["SCARD", "set_str"]));
    assert_wrongtype(&c.cmd(&["SMEMBERS", "set_str"]));
    assert_wrongtype(&c.cmd(&["SISMEMBER", "set_str", "a"]));
    assert_wrongtype(&c.cmd(&["SPOP", "set_str"]));
    c.del(&["set_str"]);
}

#[test]
#[ignore = "Session 9: set operations type checking"]
fn set_operations_against_a_wrong_type_operand() {
    // A bad key anywhere in the list fails the whole command, not just the first
    // one -- so check every key, not only argument one.
    let mut c = connect();
    c.del(&["set_1", "set_str"]);
    c.cmd(&["SADD", "set_1", "a"]);
    c.cmd(&["SET", "set_str", "v"]);
    assert_wrongtype(&c.cmd(&["SUNION", "set_1", "set_str"]));
    assert_wrongtype(&c.cmd(&["SINTER", "set_1", "set_str"]));
    assert_wrongtype(&c.cmd(&["SDIFF", "set_1", "set_str"]));
    c.del(&["set_1", "set_str"]);
}

#[test]
#[ignore = "Session 9: set commands under load"]
fn set_with_many_members() {
    // 1000 members: lookups and SMEMBERS still right, and nothing goes O(n^2).
    let mut c = connect();
    c.del(&["set_big"]);
    for i in 0..1000 {
        c.cmd(&["SADD", "set_big", &i.to_string()]);
    }
    assert_eq!(c.cmd(&["SCARD", "set_big"]), int(1000));
    assert_eq!(c.cmd(&["SISMEMBER", "set_big", "500"]), int(1));
    assert_eq!(c.cmd(&["SISMEMBER", "set_big", "1000"]), int(0));
    assert_eq!(c.cmd(&["SMEMBERS", "set_big"]).array().len(), 1000);
    c.del(&["set_big"]);
}

#[test]
#[ignore = "Session 9: integer and string members are distinct"]
fn integer_like_members_are_still_byte_strings() {
    // Members are raw bytes: "1", "01", "1.0" and " 1" are four different members.
    // Compare parsed numbers instead of bytes and this collapses to one.
    let mut c = connect();
    c.del(&["set_i"]);
    c.cmd(&["SADD", "set_i", "1", "01", "1.0", " 1"]);
    assert_eq!(c.cmd(&["SCARD", "set_i"]), int(4));
    assert_eq!(c.cmd(&["SADD", "set_i", "1"]), int(0));
    c.del(&["set_i"]);
}
