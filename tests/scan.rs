//! Ported from `redis/tests/unit/scan.tcl`.
//!
//! SCAN and its collection cousins HSCAN/SSCAN/ZSCAN. `KEYS *` walks everything
//! in one blocking command; SCAN hands out the same walk in cursor-sized chunks
//! so other clients get a turn.
//!
//! The promise is weak on purpose: a key that exists for the whole iteration is
//! returned at least once, one that never exists is never returned, one added or
//! removed midway may go either way, and keys may repeat -- so callers must
//! deduplicate and cannot count by summing batches.
//!
//! Skipped from the original: the `DEBUG` encoding checks and the
//! `SCAN ... TYPE` filter tests for streams.

mod common;
use common::*;
use std::collections::HashSet;

/// Run a full SCAN iteration and return every key seen, deduplicated.
///
/// `keys_of_scan` from the TCL suite. The loop stops when the cursor comes back
/// as "0" -- never on an empty batch, since empty batches are normal mid-scan.
fn scan_all(c: &mut Client, args: &[&str]) -> HashSet<String> {
    let mut found = HashSet::new();
    let mut cursor = "0".to_string();
    let mut iterations = 0;
    loop {
        let mut cmd = vec!["SCAN", &cursor];
        cmd.extend_from_slice(args);
        let reply = c.cmd(&cmd);
        let parts = reply.array();
        cursor = parts[0].str().to_string();
        for key in parts[1].strings() {
            found.insert(key);
        }
        iterations += 1;
        assert!(iterations < 10_000, "SCAN did not terminate");
        if cursor == "0" {
            return found;
        }
    }
}

#[test]
#[ignore = "Session 17: SCAN basic usage"]
fn scan_basic() {
    // A quiet keyspace of 1000 keys must come back complete, every key exactly
    // once after dedup.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..1000 {
        c.cmd(&["SET", &format!("scn_key:{i}"), "v"]);
    }
    let found = scan_all(&mut c, &[]);
    assert_eq!(found.len(), 1000);
    assert!(found.contains("scn_key:0"));
    assert!(found.contains("scn_key:999"));
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 17: SCAN COUNT is a hint, not a promise"]
fn scan_count() {
    // COUNT is a hint, not a promise -- a batch may hold more or fewer keys.
    // What must hold for every COUNT is that the full walk finds all 1000.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..1000 {
        c.cmd(&["SET", &format!("scn_key:{i}"), "v"]);
    }
    for count in ["1", "10", "100", "1000"] {
        assert_eq!(scan_all(&mut c, &["COUNT", count]).len(), 1000);
    }
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 17: SCAN MATCH"]
fn scan_match() {
    // MATCH filters each batch after fetching it, so a tight pattern like
    // scn_key:1?? leaves most batches empty -- another reason not to stop early.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..1000 {
        c.cmd(&["SET", &format!("scn_key:{i}"), "v"]);
    }
    c.cmd(&["SET", "scn_other", "v"]);
    let found = scan_all(&mut c, &["MATCH", "scn_key:1??"]);
    assert_eq!(found.len(), 100);
    assert!(found.contains("scn_key:100"));
    assert!(!found.contains("scn_other"));
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 17: SCAN TYPE"]
fn scan_type() {
    // TYPE list keeps only the lists: 100 back out of 200 keys.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..100 {
        c.cmd(&["SET", &format!("scn_str:{i}"), "v"]);
        c.cmd(&["RPUSH", &format!("scn_lst:{i}"), "v"]);
    }
    let found = scan_all(&mut c, &["TYPE", "list"]);
    assert_eq!(found.len(), 100);
    assert!(found.contains("scn_lst:0"));
    assert!(!found.contains("scn_str:0"));
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 17: SCAN guarantees keys present throughout are returned"]
fn scan_returns_every_key_that_was_present_throughout() {
    // The promise under mutation: 200 keys are added mid-scan, and all 500 that
    // were there the whole time must still be found. The new ones may or may not
    // show up, so nothing is asserted about them.
    let mut c = connect();
    let mut writer = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..500 {
        c.cmd(&["SET", &format!("scn_stable:{i}"), "v"]);
    }

    let mut found = HashSet::new();
    let mut cursor = "0".to_string();
    let mut added = 0;
    loop {
        let reply = c.cmd(&["SCAN", &cursor, "COUNT", "10"]);
        let parts = reply.array();
        cursor = parts[0].str().to_string();
        for key in parts[1].strings() {
            found.insert(key);
        }
        if added < 200 {
            writer.cmd(&["SET", &format!("scn_new:{added}"), "v"]);
            added += 1;
        }
        if cursor == "0" {
            break;
        }
    }

    for i in 0..500 {
        assert!(
            found.contains(&format!("scn_stable:{i}")),
            "scn_stable:{i} was missed by SCAN"
        );
    }
    c.cmd(&["FLUSHALL"]);
}

#[test]
#[ignore = "Session 17: SCAN cursor validation"]
fn scan_with_an_invalid_cursor() {
    // A non-numeric cursor is an error.
    let mut c = connect();
    assert_error(&c.cmd(&["SCAN", "notanumber"]), "ERR invalid cursor");
    // A huge but numeric cursor is fine -- it just ends the iteration. Clients
    // can legitimately hold a cursor from before the table resized.
    let reply = c.cmd(&["SCAN", "9999999999"]);
    assert_eq!(reply.array()[0].str(), "0");
}

#[test]
#[ignore = "Session 17: SCAN skips logically expired keys"]
fn scan_does_not_return_expired_keys() {
    // 100 keys live, 100 already expired => SCAN returns only the 100 live ones.
    // SCAN has to apply the same expiry check GET does, not just list the map.
    let mut c = connect();
    c.cmd(&["FLUSHALL"]);
    for i in 0..100 {
        c.cmd(&["SET", &format!("scn_alive:{i}"), "v"]);
        c.cmd(&["SET", &format!("scn_dying:{i}"), "v", "PX", "50"]);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let found = scan_all(&mut c, &[]);
    assert_eq!(found.len(), 100);
    assert!(found.iter().all(|k| k.starts_with("scn_alive:")));
    c.cmd(&["FLUSHALL"]);
}

// ---------------------------------------------------------------------------
// Collection scans
//
// Same [cursor, batch] reply shape, three different batch contents: HSCAN gives
// a flat field,value,field,value list; SSCAN gives bare members; ZSCAN gives a
// flat member,score list.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 17: HSCAN"]
fn hscan_returns_field_value_pairs() {
    // ["f1", "v1", "f2", "v2", ...] -- flat like HGETALL, not a list of fields.
    let mut c = connect();
    c.del(&["scn_h"]);
    for i in 0..100 {
        c.cmd(&["HSET", "scn_h", &format!("f{i}"), &format!("v{i}")]);
    }
    let mut fields = HashSet::new();
    let mut cursor = "0".to_string();
    loop {
        let reply = c.cmd(&["HSCAN", "scn_h", &cursor]);
        let parts = reply.array();
        cursor = parts[0].str().to_string();
        let flat = parts[1].strings();
        assert_eq!(flat.len() % 2, 0, "HSCAN must return pairs");
        for pair in flat.chunks(2) {
            assert_eq!(pair[1], pair[0].replace('f', "v"));
            fields.insert(pair[0].clone());
        }
        if cursor == "0" {
            break;
        }
    }
    assert_eq!(fields.len(), 100);
    c.del(&["scn_h"]);
}

#[test]
#[ignore = "Session 17: SSCAN"]
fn sscan_returns_members() {
    // ["m0", "m1", ...] -- members only, nothing paired with them.
    let mut c = connect();
    c.del(&["scn_s"]);
    for i in 0..100 {
        c.cmd(&["SADD", "scn_s", &format!("m{i}")]);
    }
    let mut members = HashSet::new();
    let mut cursor = "0".to_string();
    loop {
        let reply = c.cmd(&["SSCAN", "scn_s", &cursor]);
        let parts = reply.array();
        cursor = parts[0].str().to_string();
        for m in parts[1].strings() {
            members.insert(m);
        }
        if cursor == "0" {
            break;
        }
    }
    assert_eq!(members.len(), 100);
    c.del(&["scn_s"]);
}

#[test]
#[ignore = "Session 17: ZSCAN"]
fn zscan_returns_member_score_pairs() {
    // ["m0", "0", "m1", "1", ...] -- flat member/score, like ZRANGE WITHSCORES.
    let mut c = connect();
    c.del(&["scn_z"]);
    for i in 0..100 {
        c.cmd(&["ZADD", "scn_z", &i.to_string(), &format!("m{i}")]);
    }
    let mut members = HashSet::new();
    let mut cursor = "0".to_string();
    loop {
        let reply = c.cmd(&["ZSCAN", "scn_z", &cursor]);
        let parts = reply.array();
        cursor = parts[0].str().to_string();
        let flat = parts[1].strings();
        assert_eq!(flat.len() % 2, 0, "ZSCAN must return pairs");
        for pair in flat.chunks(2) {
            members.insert(pair[0].clone());
        }
        if cursor == "0" {
            break;
        }
    }
    assert_eq!(members.len(), 100);
    c.del(&["scn_z"]);
}

#[test]
#[ignore = "Session 17: collection SCAN against a missing key"]
fn collection_scans_against_a_missing_key() {
    // Missing key => ["0", []]: a finished walk over nothing, not an error.
    let mut c = connect();
    c.del(&["scn_missing"]);
    for cmd in ["HSCAN", "SSCAN", "ZSCAN"] {
        let reply = c.cmd(&[cmd, "scn_missing", "0"]);
        assert_eq!(reply.array()[0].str(), "0");
        assert_eq!(reply.array()[1], arr(vec![]));
    }
}
