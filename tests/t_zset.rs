//! Ported from `redis/tests/unit/type/zset.tcl`.
//!
//! Sorted sets: ZADD/ZSCORE, the range and rank queries, ZREM, and the store
//! commands. A zset is two views of the same data -- member -> score, and a
//! sorted (score, member) -> position -- and most failures here are the two
//! views disagreeing.
//!
//! Order is by score, then by member bytes: ZADD k 1 b, ZADD k 1 a => [a, b].
//!
//! Skipped from the original: the `listpack`/`skiplist` encoding loops, the
//! `BZPOPMIN`/`BZMPOP` blocking commands, `ZRANGESTORE`, and the `GEO*` family.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// ZADD / ZSCORE / ZCARD
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 10: zset commands (ZADD/ZSCORE)"]
fn zadd_and_zscore_basic() {
    // ZADD counts new members only: re-adding "a" with a new score returns 0
    // but still changes the score.
    let mut c = connect();
    c.del(&["zst_z"]);
    assert_eq!(c.cmd(&["ZADD", "zst_z", "1", "a"]), int(1));
    assert_eq!(c.cmd(&["ZADD", "zst_z", "2", "b", "3", "c"]), int(2));
    assert_eq!(c.cmd(&["ZADD", "zst_z", "10", "a"]), int(0), "update, not add");
    assert_eq!(c.cmd(&["ZSCORE", "zst_z", "a"]), bulk("10"));
    assert_eq!(c.cmd(&["ZCARD", "zst_z"]), int(3));
    c.del(&["zst_z"]);
}

#[test]
#[ignore = "Session 10: zset commands (ZSCORE against missing member/key)"]
fn zscore_against_non_existing_member_or_key() {
    // Missing member and missing key both give nil -- no error, no 0.
    let mut c = connect();
    c.del(&["zst_z"]);
    c.cmd(&["ZADD", "zst_z", "1", "a"]);
    assert!(c.cmd(&["ZSCORE", "zst_z", "nomember"]).is_nil());
    c.del(&["zst_missing"]);
    assert!(c.cmd(&["ZSCORE", "zst_missing", "a"]).is_nil());
    c.del(&["zst_z"]);
}

#[test]
#[ignore = "Session 10: zset score formatting"]
fn scores_are_returned_as_strings_without_trailing_zeros() {
    // Scores are f64 but come back as strings, with no trailing ".0":
    // 3 => "3", 1.5 => "1.5", 3.0e3 => "3000".
    let mut c = connect();
    c.del(&["zst_f"]);
    c.cmd(&["ZADD", "zst_f", "3", "a", "1.5", "b", "-1", "c", "3.0e3", "d"]);
    assert_eq!(c.cmd(&["ZSCORE", "zst_f", "a"]), bulk("3"));
    assert_eq!(c.cmd(&["ZSCORE", "zst_f", "b"]), bulk("1.5"));
    assert_eq!(c.cmd(&["ZSCORE", "zst_f", "c"]), bulk("-1"));
    assert_eq!(c.cmd(&["ZSCORE", "zst_f", "d"]), bulk("3000"));
    c.del(&["zst_f"]);
}

#[test]
#[ignore = "Session 10: zset accepts inf scores but rejects nan"]
fn zadd_accepts_infinity_and_rejects_nan() {
    // +inf/-inf are real scores and sort at the ends. NaN is rejected at parse
    // time -- it compares equal to nothing, so it would corrupt the sorted view.
    let mut c = connect();
    c.del(&["zst_inf"]);
    assert_eq!(c.cmd(&["ZADD", "zst_inf", "+inf", "a", "-inf", "b"]), int(2));
    assert_eq!(c.cmd(&["ZSCORE", "zst_inf", "a"]), bulk("inf"));
    assert_eq!(c.cmd(&["ZSCORE", "zst_inf", "b"]), bulk("-inf"));
    assert_eq!(c.cmd(&["ZRANGE", "zst_inf", "0", "-1"]), bulks(&["b", "a"]));

    assert_error(&c.cmd(&["ZADD", "zst_inf", "nan", "c"]), "ERR value is not a valid float");
    assert_error(&c.cmd(&["ZADD", "zst_inf", "notanumber", "c"]), "ERR value is not a valid float");
    c.del(&["zst_inf"]);
}

#[test]
#[ignore = "Session 10: zset NaN cannot be produced by arithmetic"]
fn zincrby_cannot_produce_nan() {
    // inf + -inf = NaN, so check the result too, not just the argument.
    let mut c = connect();
    c.del(&["zst_nan"]);
    c.cmd(&["ZADD", "zst_nan", "+inf", "a"]);
    assert_error(&c.cmd(&["ZINCRBY", "zst_nan", "-inf", "a"]), "ERR resulting score is not a number");
    c.del(&["zst_nan"]);
}

#[test]
#[ignore = "Session 10: zset ties order lexicographically by member"]
fn zadd_ties_are_ordered_lexicographically() {
    // Same score => order by member bytes. Insert e,d,c,b,a => read back a..e.
    // This is why the sorted key must be (score, member), not score alone --
    // a Vec of members per score would keep insertion order and fail here.
    let mut c = connect();
    c.del(&["zst_tie"]);
    c.cmd(&["ZADD", "zst_tie", "1", "e", "1", "d", "1", "c", "1", "b", "1", "a"]);
    assert_eq!(
        c.cmd(&["ZRANGE", "zst_tie", "0", "-1"]),
        bulks(&["a", "b", "c", "d", "e"])
    );
    assert_eq!(c.cmd(&["ZRANK", "zst_tie", "a"]), int(0));
    assert_eq!(c.cmd(&["ZRANK", "zst_tie", "e"]), int(4));
    c.del(&["zst_tie"]);
}

#[test]
#[ignore = "Session 10: updating a score must reposition the member"]
fn updating_a_score_moves_the_member_in_the_ordering() {
    // a=1 becomes a=10, so [a,b,c] becomes [b,c,a] and ZCARD stays 3.
    // Miss the removal of the old (1,"a") entry and a ghost shows up in ZRANGE.
    let mut c = connect();
    c.del(&["zst_u"]);
    c.cmd(&["ZADD", "zst_u", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZRANGE", "zst_u", "0", "-1"]), bulks(&["a", "b", "c"]));
    c.cmd(&["ZADD", "zst_u", "10", "a"]);
    assert_eq!(c.cmd(&["ZRANGE", "zst_u", "0", "-1"]), bulks(&["b", "c", "a"]));
    assert_eq!(c.cmd(&["ZCARD", "zst_u"]), int(3), "no duplicate left behind");
    c.del(&["zst_u"]);
}

#[test]
#[ignore = "Session 10: ZADD NX/XX/GT/LT/CH/INCR options"]
fn zadd_options() {
    // The flags change what ZADD does and what it replies. Combinations that
    // contradict each other (NX+XX, NX+GT) are errors.
    let mut c = connect();

    // NX: only add new members, never update.
    c.del(&["zst_o"]);
    c.cmd(&["ZADD", "zst_o", "1", "a"]);
    assert_eq!(c.cmd(&["ZADD", "zst_o", "NX", "5", "a"]), int(0));
    assert_eq!(c.cmd(&["ZSCORE", "zst_o", "a"]), bulk("1"));

    // XX: only update existing members, never add.
    assert_eq!(c.cmd(&["ZADD", "zst_o", "XX", "5", "newmember"]), int(0));
    assert_eq!(c.cmd(&["ZSCORE", "zst_o", "newmember"]), nil());
    c.cmd(&["ZADD", "zst_o", "XX", "5", "a"]);
    assert_eq!(c.cmd(&["ZSCORE", "zst_o", "a"]), bulk("5"));

    // CH counts changed members too, not just new ones.
    assert_eq!(c.cmd(&["ZADD", "zst_o", "CH", "9", "a"]), int(1));
    assert_eq!(c.cmd(&["ZADD", "zst_o", "CH", "9", "a"]), int(0), "unchanged");

    // GT/LT only move the score in one direction.
    c.cmd(&["ZADD", "zst_o", "5", "b"]);
    c.cmd(&["ZADD", "zst_o", "GT", "3", "b"]);
    assert_eq!(c.cmd(&["ZSCORE", "zst_o", "b"]), bulk("5"), "GT must not lower");
    c.cmd(&["ZADD", "zst_o", "GT", "7", "b"]);
    assert_eq!(c.cmd(&["ZSCORE", "zst_o", "b"]), bulk("7"));

    // INCR: acts like ZINCRBY and replies with the new score, not a count.
    c.del(&["zst_o2"]);
    assert_eq!(c.cmd(&["ZADD", "zst_o2", "INCR", "5", "a"]), bulk("5"));
    assert_eq!(c.cmd(&["ZADD", "zst_o2", "INCR", "5", "a"]), bulk("10"));
    // INCR with NX on an existing member does nothing and returns nil.
    assert!(c.cmd(&["ZADD", "zst_o2", "NX", "INCR", "5", "a"]).is_nil());

    assert_error(&c.cmd(&["ZADD", "zst_o", "NX", "XX", "1", "a"]), "ERR");
    assert_error(&c.cmd(&["ZADD", "zst_o", "GT", "NX", "1", "a"]), "ERR");
    c.del(&["zst_o", "zst_o2"]);
}

#[test]
#[ignore = "Session 10: zset commands (ZINCRBY)"]
fn zincrby_against_new_and_existing_members() {
    // ZINCRBY on a missing member starts from 0, so the first call returns "1".
    let mut c = connect();
    c.del(&["zst_i"]);
    assert_eq!(c.cmd(&["ZINCRBY", "zst_i", "1", "a"]), bulk("1"));
    assert_eq!(c.cmd(&["ZINCRBY", "zst_i", "2", "a"]), bulk("3"));
    assert_eq!(c.cmd(&["ZINCRBY", "zst_i", "-5", "a"]), bulk("-2"));
    c.del(&["zst_i"]);
}

// ---------------------------------------------------------------------------
// ZRANGE / ZREVRANGE / ZRANK
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 10: zset commands (ZRANGE)"]
fn zrange_by_index() {
    // Indexes, not scores. Negatives count from the end (-1 is the last), and an
    // out-of-range or missing key gives an empty array, never an error.
    let mut c = connect();
    c.del(&["zst_r"]);
    c.cmd(&["ZADD", "zst_r", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZRANGE", "zst_r", "0", "-1"]), bulks(&["a", "b", "c"]));
    assert_eq!(c.cmd(&["ZRANGE", "zst_r", "0", "1"]), bulks(&["a", "b"]));
    assert_eq!(c.cmd(&["ZRANGE", "zst_r", "-2", "-1"]), bulks(&["b", "c"]));
    assert_eq!(c.cmd(&["ZRANGE", "zst_r", "5", "10"]), arr(vec![]));
    c.del(&["zst_missing"]);
    assert_eq!(c.cmd(&["ZRANGE", "zst_missing", "0", "-1"]), arr(vec![]));
    c.del(&["zst_r"]);
}

#[test]
#[ignore = "Session 10: ZRANGE WITHSCORES"]
fn zrange_withscores_interleaves_member_and_score() {
    // Flat array like HGETALL: member, score, member, score. Not pairs.
    let mut c = connect();
    c.del(&["zst_r"]);
    c.cmd(&["ZADD", "zst_r", "1", "a", "2", "b"]);
    assert_eq!(
        c.cmd(&["ZRANGE", "zst_r", "0", "-1", "WITHSCORES"]),
        bulks(&["a", "1", "b", "2"])
    );
    c.del(&["zst_r"]);
}

#[test]
#[ignore = "Session 10: ZREVRANGE"]
fn zrevrange_reverses_the_ordering_including_ties() {
    // Reverse the whole comparison, member order included: with all scores 1,
    // ZREVRANGE gives [c, b, a].
    let mut c = connect();
    c.del(&["zst_rv"]);
    c.cmd(&["ZADD", "zst_rv", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZREVRANGE", "zst_rv", "0", "-1"]), bulks(&["c", "b", "a"]));
    assert_eq!(c.cmd(&["ZREVRANGE", "zst_rv", "0", "1"]), bulks(&["c", "b"]));

    c.del(&["zst_rv"]);
    c.cmd(&["ZADD", "zst_rv", "1", "a", "1", "b", "1", "c"]);
    assert_eq!(c.cmd(&["ZREVRANGE", "zst_rv", "0", "-1"]), bulks(&["c", "b", "a"]));
    c.del(&["zst_rv"]);
}

#[test]
#[ignore = "Session 10: zset commands (ZRANK/ZREVRANK)"]
fn zrank_and_zrevrank() {
    // Ranks are 0-based, and ZREVRANK counts from the other end.
    // A missing member is nil, not -1 -- any integer could be a real rank.
    let mut c = connect();
    c.del(&["zst_k"]);
    c.cmd(&["ZADD", "zst_k", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZRANK", "zst_k", "a"]), int(0));
    assert_eq!(c.cmd(&["ZRANK", "zst_k", "c"]), int(2));
    assert_eq!(c.cmd(&["ZREVRANK", "zst_k", "a"]), int(2));
    assert_eq!(c.cmd(&["ZREVRANK", "zst_k", "c"]), int(0));
    assert!(c.cmd(&["ZRANK", "zst_k", "nomember"]).is_nil());
    c.del(&["zst_missing"]);
    assert!(c.cmd(&["ZRANK", "zst_missing", "a"]).is_nil());
    c.del(&["zst_k"]);
}

// ---------------------------------------------------------------------------
// ZRANGEBYSCORE / ZCOUNT
//
// Score bounds: `3` is inclusive, `(3` is exclusive, `-inf`/`+inf` are the open
// ends. ZCOUNT and ZREMRANGEBYSCORE take the same syntax, so parse it in one
// shared helper. ZRANGEBYLEX has its own syntax -- see that test.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: ZRANGEBYSCORE"]
fn zrangebyscore_inclusive_exclusive_and_infinite_bounds() {
    // min > max is an empty array, not an error. A non-numeric bound is an error.
    let mut c = connect();
    c.del(&["zst_s"]);
    c.cmd(&["ZADD", "zst_s", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(
        c.cmd(&["ZRANGEBYSCORE", "zst_s", "-inf", "+inf"]),
        bulks(&["a", "b", "c"])
    );
    assert_eq!(c.cmd(&["ZRANGEBYSCORE", "zst_s", "1", "2"]), bulks(&["a", "b"]));
    assert_eq!(c.cmd(&["ZRANGEBYSCORE", "zst_s", "(1", "3"]), bulks(&["b", "c"]));
    assert_eq!(c.cmd(&["ZRANGEBYSCORE", "zst_s", "(1", "(3"]), bulks(&["b"]));
    assert_eq!(c.cmd(&["ZRANGEBYSCORE", "zst_s", "3", "1"]), arr(vec![]));
    assert_error(&c.cmd(&["ZRANGEBYSCORE", "zst_s", "notafloat", "3"]), "ERR min or max is not a float");
    c.del(&["zst_s"]);
}

#[test]
#[ignore = "bonus: ZRANGEBYSCORE LIMIT"]
fn zrangebyscore_with_limit() {
    // LIMIT offset count, like SQL: LIMIT 1 2 skips one then takes two.
    let mut c = connect();
    c.del(&["zst_s"]);
    c.cmd(&["ZADD", "zst_s", "1", "a", "2", "b", "3", "c", "4", "d"]);
    assert_eq!(
        c.cmd(&["ZRANGEBYSCORE", "zst_s", "-inf", "+inf", "LIMIT", "1", "2"]),
        bulks(&["b", "c"])
    );
    // count -1 means "all the rest".
    assert_eq!(
        c.cmd(&["ZRANGEBYSCORE", "zst_s", "-inf", "+inf", "LIMIT", "2", "-1"]),
        bulks(&["c", "d"])
    );
    c.del(&["zst_s"]);
}

#[test]
#[ignore = "bonus: ZCOUNT"]
fn zcount_uses_the_same_bound_syntax() {
    // Same bounds as ZRANGEBYSCORE, but counts instead of listing.
    // A missing key counts 0.
    let mut c = connect();
    c.del(&["zst_ct"]);
    c.cmd(&["ZADD", "zst_ct", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZCOUNT", "zst_ct", "-inf", "+inf"]), int(3));
    assert_eq!(c.cmd(&["ZCOUNT", "zst_ct", "1", "2"]), int(2));
    assert_eq!(c.cmd(&["ZCOUNT", "zst_ct", "(1", "3"]), int(2));
    c.del(&["zst_missing"]);
    assert_eq!(c.cmd(&["ZCOUNT", "zst_missing", "-inf", "+inf"]), int(0));
    c.del(&["zst_ct"]);
}

#[test]
#[ignore = "bonus: ZRANGEBYLEX"]
fn zrangebylex_requires_identical_scores() {
    // Only meaningful when all scores are equal -- then the order is the member
    // order. Different bound syntax: `[b` inclusive, `(b` exclusive, `-`/`+` for
    // the ends. A bare `b` is an error.
    let mut c = connect();
    c.del(&["zst_lex"]);
    c.cmd(&["ZADD", "zst_lex", "0", "a", "0", "b", "0", "c", "0", "d"]);
    assert_eq!(c.cmd(&["ZRANGEBYLEX", "zst_lex", "-", "+"]), bulks(&["a", "b", "c", "d"]));
    assert_eq!(c.cmd(&["ZRANGEBYLEX", "zst_lex", "[b", "[c"]), bulks(&["b", "c"]));
    assert_eq!(c.cmd(&["ZRANGEBYLEX", "zst_lex", "(b", "+"]), bulks(&["c", "d"]));
    assert_error(&c.cmd(&["ZRANGEBYLEX", "zst_lex", "b", "c"]), "ERR min or max not valid string range item");
    c.del(&["zst_lex"]);
}

// ---------------------------------------------------------------------------
// ZREM and the ZREMRANGE family
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 10: zset commands (ZREM)"]
fn zrem_removes_members_and_counts_them() {
    // ZREM counts only members that were there. Removing the last one deletes
    // the key, so EXISTS goes to 0 -- Redis keeps no empty collections.
    let mut c = connect();
    c.del(&["zst_rm"]);
    c.cmd(&["ZADD", "zst_rm", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZREM", "zst_rm", "a"]), int(1));
    assert_eq!(c.cmd(&["ZREM", "zst_rm", "nomember"]), int(0));
    assert_eq!(c.cmd(&["ZREM", "zst_rm", "b", "c", "nomember"]), int(2));
    assert_eq!(c.cmd(&["EXISTS", "zst_rm"]), int(0), "emptied zsets are deleted");
}

#[test]
#[ignore = "bonus: ZREMRANGEBYRANK / BYSCORE / BYLEX"]
fn zremrange_variants() {
    // Same three ways of naming a range as the ZRANGE family, reused to delete.
    let mut c = connect();

    c.del(&["zst_rr"]);
    c.cmd(&["ZADD", "zst_rr", "1", "a", "2", "b", "3", "c", "4", "d"]);
    assert_eq!(c.cmd(&["ZREMRANGEBYRANK", "zst_rr", "0", "1"]), int(2));
    assert_eq!(c.cmd(&["ZRANGE", "zst_rr", "0", "-1"]), bulks(&["c", "d"]));

    c.del(&["zst_rr"]);
    c.cmd(&["ZADD", "zst_rr", "1", "a", "2", "b", "3", "c", "4", "d"]);
    assert_eq!(c.cmd(&["ZREMRANGEBYSCORE", "zst_rr", "(1", "3"]), int(2));
    assert_eq!(c.cmd(&["ZRANGE", "zst_rr", "0", "-1"]), bulks(&["a", "d"]));

    // Removing everything deletes the key.
    assert_eq!(c.cmd(&["ZREMRANGEBYSCORE", "zst_rr", "-inf", "+inf"]), int(2));
    assert_eq!(c.cmd(&["EXISTS", "zst_rr"]), int(0));
}

#[test]
#[ignore = "bonus: ZPOPMIN / ZPOPMAX"]
fn zpopmin_and_zpopmax() {
    // Flat member+score reply: ZPOPMIN => ["a", "1"], ZPOPMIN k 2 => 4 elements.
    // Popping from a missing key is an empty array, not nil.
    let mut c = connect();
    c.del(&["zst_p"]);
    c.cmd(&["ZADD", "zst_p", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZPOPMIN", "zst_p"]), bulks(&["a", "1"]));
    assert_eq!(c.cmd(&["ZPOPMAX", "zst_p"]), bulks(&["c", "3"]));
    assert_eq!(c.cmd(&["ZCARD", "zst_p"]), int(1));

    c.del(&["zst_p"]);
    c.cmd(&["ZADD", "zst_p", "1", "a", "2", "b", "3", "c"]);
    assert_eq!(c.cmd(&["ZPOPMIN", "zst_p", "2"]), bulks(&["a", "1", "b", "2"]));

    c.del(&["zst_missing"]);
    assert_eq!(c.cmd(&["ZPOPMIN", "zst_missing"]), arr(vec![]));
    c.del(&["zst_p"]);
}

// ---------------------------------------------------------------------------
// ZUNIONSTORE / ZINTERSTORE
// ---------------------------------------------------------------------------

#[test]
#[ignore = "bonus: ZUNIONSTORE / ZINTERSTORE"]
fn zunionstore_and_zinterstore_aggregate_scores() {
    // Members in both inputs get their scores summed by default: b is 2 and 10,
    // so it lands on 12; AGGREGATE MAX gives 10 instead. The `numkeys` argument
    // is required -- it is how the parser knows where the keys stop.
    let mut c = connect();
    c.del(&["zst_1", "zst_2", "zst_dst"]);
    c.cmd(&["ZADD", "zst_1", "1", "a", "2", "b"]);
    c.cmd(&["ZADD", "zst_2", "10", "b", "20", "c"]);

    assert_eq!(c.cmd(&["ZUNIONSTORE", "zst_dst", "2", "zst_1", "zst_2"]), int(3));
    assert_eq!(c.cmd(&["ZSCORE", "zst_dst", "b"]), bulk("12"));

    assert_eq!(c.cmd(&["ZINTERSTORE", "zst_dst", "2", "zst_1", "zst_2"]), int(1));
    assert_eq!(c.cmd(&["ZRANGE", "zst_dst", "0", "-1"]), bulks(&["b"]));

    assert_eq!(
        c.cmd(&["ZUNIONSTORE", "zst_dst", "2", "zst_1", "zst_2", "AGGREGATE", "MAX"]),
        int(3)
    );
    assert_eq!(c.cmd(&["ZSCORE", "zst_dst", "b"]), bulk("10"));
    c.del(&["zst_1", "zst_2", "zst_dst"]);
}

#[test]
#[ignore = "bonus: ZUNIONSTORE WEIGHTS"]
fn zunionstore_with_weights() {
    // Each input score is multiplied by its weight before aggregating:
    // 1*2 + 1*3 = 5. One weight per key, or it is a syntax error.
    let mut c = connect();
    c.del(&["zst_1", "zst_2", "zst_dst"]);
    c.cmd(&["ZADD", "zst_1", "1", "a"]);
    c.cmd(&["ZADD", "zst_2", "1", "a"]);
    c.cmd(&["ZUNIONSTORE", "zst_dst", "2", "zst_1", "zst_2", "WEIGHTS", "2", "3"]);
    assert_eq!(c.cmd(&["ZSCORE", "zst_dst", "a"]), bulk("5"));
    assert_error(
        &c.cmd(&["ZUNIONSTORE", "zst_dst", "2", "zst_1", "zst_2", "WEIGHTS", "2"]),
        "ERR syntax error",
    );
    c.del(&["zst_1", "zst_2", "zst_dst"]);
}

#[test]
#[ignore = "bonus: zset store operations accept sets as input"]
fn zset_store_operations_treat_a_set_as_scores_of_one() {
    // A plain set is a legal input; every member scores 1. So a=5 in the zset
    // plus a in the set gives 6. The type check here is "set or zset".
    let mut c = connect();
    c.del(&["zst_1", "set_1", "zst_dst"]);
    c.cmd(&["ZADD", "zst_1", "5", "a"]);
    c.cmd(&["SADD", "set_1", "a", "b"]);
    c.cmd(&["ZUNIONSTORE", "zst_dst", "2", "zst_1", "set_1"]);
    assert_eq!(c.cmd(&["ZSCORE", "zst_dst", "a"]), bulk("6"));
    assert_eq!(c.cmd(&["ZSCORE", "zst_dst", "b"]), bulk("1"));
    c.del(&["zst_1", "set_1", "zst_dst"]);
}

// ---------------------------------------------------------------------------
// Type errors and volume
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 10: zset commands type checking"]
fn zset_commands_against_a_string_key() {
    // Every zset command on a string key must reply WRONGTYPE -- reads included,
    // so the type check belongs before the read, not only before the write.
    let mut c = connect();
    c.del(&["zst_str"]);
    c.cmd(&["SET", "zst_str", "v"]);
    assert_wrongtype(&c.cmd(&["ZADD", "zst_str", "1", "a"]));
    assert_wrongtype(&c.cmd(&["ZSCORE", "zst_str", "a"]));
    assert_wrongtype(&c.cmd(&["ZCARD", "zst_str"]));
    assert_wrongtype(&c.cmd(&["ZRANGE", "zst_str", "0", "-1"]));
    assert_wrongtype(&c.cmd(&["ZRANK", "zst_str", "a"]));
    assert_wrongtype(&c.cmd(&["ZREM", "zst_str", "a"]));
    assert_wrongtype(&c.cmd(&["ZINCRBY", "zst_str", "1", "a"]));
    c.del(&["zst_str"]);
}

#[test]
#[ignore = "Session 10: zset ordering under load"]
fn large_zset_keeps_a_consistent_ordering() {
    // 1000 members inserted out of order, with many ties, read back sorted.
    // Catches the two views drifting apart, which three members never would.
    let mut c = connect();
    c.del(&["zst_big"]);
    for i in 0..1000 {
        let score = (i * 7919) % 1000; // deterministic, non-sequential
        c.cmd(&["ZADD", "zst_big", &score.to_string(), &format!("m{i:04}")]);
    }
    assert_eq!(c.cmd(&["ZCARD", "zst_big"]), int(1000));

    let members = c.cmd(&["ZRANGE", "zst_big", "0", "-1", "WITHSCORES"]).strings();
    assert_eq!(members.len(), 2000);
    let mut previous: Option<(f64, String)> = None;
    for pair in members.chunks(2) {
        let score: f64 = pair[1].parse().unwrap();
        let member = pair[0].clone();
        if let Some((prev_score, prev_member)) = previous {
            assert!(
                score > prev_score || (score == prev_score && member > prev_member),
                "ordering violated at {member}/{score} after {prev_member}/{prev_score}"
            );
        }
        previous = Some((score, member));
    }
    c.del(&["zst_big"]);
}
