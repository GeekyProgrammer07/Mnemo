//! AOF persistence, ported from `redis/tests/unit/aofrw.tcl`,
//! `tests/integration/aof.tcl` and `tests/integration/aof-multi-part.tcl`.
//!
//! An AOF is a log of every write command in RESP, appended as it happens;
//! recovery replays it against an empty store. These tests write, restart the
//! server, and check the data came back.
//!
//! Skipped from those files: `BGREWRITEAOF`, the multi-part manifest format,
//! `aof-use-rdb-preamble`, and everything about replication. The roadmap builds
//! the original single-file journal.
//!
//! # This file runs its own server
//!
//! Other test files share one long-lived server; these restart theirs, so they
//! must not run alongside the rest:
//!
//! ```text
//! cargo test --test persistence -- --ignored --test-threads=1
//! ```

mod common;
use common::*;

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A server in its own scratch directory that we can stop and start.
struct AofServer {
    child: Option<Child>,
    dir: PathBuf,
}

impl AofServer {
    fn start() -> AofServer {
        let dir = std::env::temp_dir().join(format!("mnemo-aof-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("could not create the scratch directory");
        let mut server = AofServer { child: None, dir };
        server.spawn();
        server
    }

    /// Boot the binary and wait until it accepts connections.
    fn spawn(&mut self) {
        assert!(self.child.is_none(), "server is already running");
        // A leftover process from an earlier run would still hold the port.
        let _ = Command::new("pkill").args(["-f", "mnemo"]).status();
        wait_for_port(false);

        let child = Command::new(env!("CARGO_BIN_EXE_mnemo"))
            .current_dir(&self.dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn the mnemo binary");
        self.child = Some(child);
        wait_for_port(true);
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        wait_for_port(false);
    }

    /// Stop and start again -- what every test here is built around.
    fn restart(&mut self) {
        self.stop();
        self.spawn();
    }

    fn client(&self) -> Client {
        Client::connect(PORT)
    }

    fn aof_path(&self) -> PathBuf {
        self.dir.join("appendonly.aof")
    }

    fn aof_contents(&self) -> String {
        std::fs::read_to_string(self.aof_path()).unwrap_or_default()
    }
}

impl Drop for AofServer {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Block until the port is (or is not) accepting connections.
fn wait_for_port(should_be_up: bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", PORT)).is_ok() == should_be_up {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for the port to be {}", if should_be_up { "up" } else { "down" });
}

/// Count how many times a command name appears in the AOF.
fn count_command(aof: &str, name: &str) -> usize {
    BufReader::new(aof.as_bytes())
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.eq_ignore_ascii_case(name))
        .count()
}

// ---------------------------------------------------------------------------
// Write path
//
// What lands in the file: writes that actually changed something, logged after
// they succeed. Nothing else.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 12: AOF write path"]
fn writes_are_appended_to_the_aof() {
    // Two `SET`s go in, two `SET`s come out in the file.
    let server = AofServer::start();
    let mut c = server.client();
    c.cmd(&["SET", "aof_k", "v"]);
    c.cmd(&["SET", "aof_k2", "v2"]);

    let aof = server.aof_contents();
    assert!(!aof.is_empty(), "the AOF should exist and have content");
    assert_eq!(count_command(&aof, "SET"), 2);
    assert!(aof.contains("aof_k"), "the key should appear in the log");
}

#[test]
#[ignore = "Session 12: the AOF holds RESP-encoded commands"]
fn the_aof_is_resp_encoded() {
    // The file holds exactly what a client would send:
    // `*3\r\n$3\r\nSET\r\n$5\r\naof_k\r\n$5\r\nhello\r\n`. Same format means the
    // parser you already have reads it back.
    let server = AofServer::start();
    let mut c = server.client();
    c.cmd(&["SET", "aof_k", "hello"]);

    let aof = server.aof_contents();
    assert!(
        aof.contains("*3\r\n$3\r\nSET\r\n$5\r\naof_k\r\n$5\r\nhello\r\n"),
        "expected a RESP array in the AOF, got:\n{aof}"
    );
}

#[test]
#[ignore = "Session 12: reads are not logged"]
fn reads_are_not_appended_to_the_aof() {
    // 400 reads later the file is the same size. Logging `GET` would grow it
    // forever and change nothing on replay.
    let server = AofServer::start();
    let mut c = server.client();
    c.cmd(&["SET", "aof_k", "v"]);
    let before = server.aof_contents().len();
    for _ in 0..100 {
        c.cmd(&["GET", "aof_k"]);
        c.cmd(&["EXISTS", "aof_k"]);
        c.cmd(&["TTL", "aof_k"]);
        c.cmd(&["PING"]);
    }
    assert_eq!(server.aof_contents().len(), before, "reads must not grow the AOF");
}

#[test]
#[ignore = "Session 12: failed commands are not logged"]
fn failed_commands_are_not_appended_to_the_aof() {
    // Log after success, never before: an `INCR` on a list fails, so it must not
    // be in the file to fail again on every future startup.
    let server = AofServer::start();
    let mut c = server.client();
    c.cmd(&["RPUSH", "aof_list", "a"]);
    let before = server.aof_contents().len();

    assert_wrongtype(&c.cmd(&["INCR", "aof_list"]));
    assert_error(&c.cmd(&["SET", "aof_x", "v", "EX", "0"]), "ERR");
    assert_error(&c.cmd(&["NONEXISTINGCOMMAND"]), "ERR");

    assert_eq!(server.aof_contents().len(), before);
}

#[test]
#[ignore = "Session 12: no-op writes are not logged"]
fn writes_that_changed_nothing_are_not_appended() {
    // `SET k v NX` on a key that exists, `DEL` of a missing key, `SREM` of an
    // absent member: all fine, all changed nothing, so none get logged.
    let server = AofServer::start();
    let mut c = server.client();
    c.cmd(&["SET", "aof_k", "v"]);
    let before = server.aof_contents().len();

    assert!(c.cmd(&["SET", "aof_k", "other", "NX"]).is_nil());
    assert_eq!(c.cmd(&["DEL", "aof_missing"]), int(0));
    assert_eq!(c.cmd(&["SREM", "aof_missing", "m"]), int(0));

    assert_eq!(server.aof_contents().len(), before);
}

// ---------------------------------------------------------------------------
// Reload
//
// Kill the server, start it again, and check the keyspace is exactly what it
// was -- replaying the log, not guessing at it.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 13: AOF reload restores strings"]
fn string_data_survives_a_restart() {
    // Replay must apply commands in order: two `INCR aof_counter` => "2", and
    // `APPEND` after `SET` => "value2-appended".
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        c.cmd(&["SET", "aof_s1", "value1"]);
        c.cmd(&["SET", "aof_s2", "value2"]);
        c.cmd(&["APPEND", "aof_s2", "-appended"]);
        c.cmd(&["INCR", "aof_counter"]);
        c.cmd(&["INCR", "aof_counter"]);
    }
    server.restart();
    let mut c = server.client();
    assert_eq!(c.cmd(&["GET", "aof_s1"]), bulk("value1"));
    assert_eq!(c.cmd(&["GET", "aof_s2"]), bulk("value2-appended"));
    assert_eq!(c.cmd(&["GET", "aof_counter"]), bulk("2"));
}

#[test]
#[ignore = "Session 13: AOF reload restores every type"]
fn all_value_types_survive_a_restart() {
    // Same check for list, hash, set and sorted set -- including zset scores.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        c.cmd(&["SET", "aof_str", "v"]);
        c.cmd(&["RPUSH", "aof_list", "a", "b", "c"]);
        c.cmd(&["HSET", "aof_hash", "f1", "v1", "f2", "v2"]);
        c.cmd(&["SADD", "aof_set", "x", "y", "z"]);
        c.cmd(&["ZADD", "aof_zset", "1", "a", "2", "b"]);
    }
    server.restart();
    let mut c = server.client();
    assert_eq!(c.cmd(&["GET", "aof_str"]), bulk("v"));
    assert_eq!(c.cmd(&["LRANGE", "aof_list", "0", "-1"]), bulks(&["a", "b", "c"]));
    assert_eq!(c.cmd(&["HGET", "aof_hash", "f2"]), bulk("v2"));
    assert_eq!(c.cmd(&["SMEMBERS", "aof_set"]).sorted(), vec!["x", "y", "z"]);
    assert_eq!(c.cmd(&["ZRANGE", "aof_zset", "0", "-1"]), bulks(&["a", "b"]));
    assert_eq!(c.cmd(&["ZSCORE", "aof_zset", "b"]), bulk("2"));
}

#[test]
#[ignore = "Session 13: deletions survive a restart"]
fn deleted_keys_stay_deleted_after_a_restart() {
    // `DEL` is a write too. Leave it out of the log and the key comes back from
    // the dead on the next restart.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        c.cmd(&["SET", "aof_gone", "v"]);
        c.cmd(&["SET", "aof_stays", "v"]);
        c.cmd(&["DEL", "aof_gone"]);
    }
    server.restart();
    let mut c = server.client();
    assert_eq!(c.cmd(&["EXISTS", "aof_gone"]), int(0));
    assert_eq!(c.cmd(&["EXISTS", "aof_stays"]), int(1));
}

#[test]
#[ignore = "Session 13: an empty or missing AOF starts an empty server"]
fn a_missing_aof_is_not_an_error() {
    // First boot has no `appendonly.aof`. No file means an empty store, not an
    // error -- otherwise the server never starts the first time.
    let mut server = AofServer::start();
    let mut c = server.client();
    assert_eq!(c.cmd(&["DBSIZE"]), int(0));
    assert_eq!(c.cmd(&["PING"]), simple("PONG"));
    server.restart();
    let mut c = server.client();
    assert_eq!(c.cmd(&["DBSIZE"]), int(0));
}

#[test]
#[ignore = "Session 13: reload is idempotent"]
fn repeated_restarts_do_not_duplicate_data() {
    // Three restarts, still 3 list items and 1 key. If the loader logs what it
    // replays, the file doubles every boot -- turn logging off while loading.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        c.cmd(&["RPUSH", "aof_list", "a", "b", "c"]);
    }
    for _ in 0..3 {
        server.restart();
        let mut c = server.client();
        assert_eq!(c.cmd(&["LLEN", "aof_list"]), int(3));
        assert_eq!(c.cmd(&["DBSIZE"]), int(1));
    }
}

// ---------------------------------------------------------------------------
// Determinism
//
// A command whose result depends on the clock or on chance must be logged as
// what it DID, not what was asked. Replaying the original gives a different
// answer the second time.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 13: TTLs are logged as absolute timestamps"]
fn expiry_is_logged_as_an_absolute_time() {
    // Log `EXPIRE k 100` as-is and every restart restarts the 100 seconds, so
    // the key never expires. Log `PEXPIREAT k <deadline>` instead -- an absolute
    // timestamp, not a duration.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        c.cmd(&["SET", "aof_vol", "v"]);
        c.cmd(&["EXPIRE", "aof_vol", "100"]);
    }
    let aof = server.aof_contents();
    assert!(
        aof.to_uppercase().contains("PEXPIREAT"),
        "EXPIRE should be logged as PEXPIREAT, got:\n{aof}"
    );

    std::thread::sleep(Duration::from_millis(1100));
    server.restart();
    let mut c = server.client();
    let ttl = c.cmd(&["TTL", "aof_vol"]).int();
    assert!(
        ttl > 90 && ttl < 100,
        "the TTL must keep counting down across a restart, got {ttl}"
    );
}

#[test]
#[ignore = "Session 13: already-expired keys are not reloaded"]
fn keys_that_expired_while_the_server_was_down_do_not_come_back() {
    // A 300ms TTL that ran out during the downtime must not be reloaded; the
    // 1000s one must be. The loader checks each deadline against now.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        c.cmd(&["SET", "aof_short", "v", "PX", "300"]);
        c.cmd(&["SET", "aof_long", "v", "EX", "1000"]);
    }
    std::thread::sleep(Duration::from_millis(600));
    server.restart();
    let mut c = server.client();
    assert_eq!(c.cmd(&["EXISTS", "aof_short"]), int(0));
    assert_eq!(c.cmd(&["EXISTS", "aof_long"]), int(1));
}

#[test]
#[ignore = "Session 13: random commands are logged by their effect"]
fn spop_is_logged_as_the_member_it_actually_removed() {
    // `SPOP` picks at random, so replaying it removes a different member and the
    // restored set silently differs. Log `SREM <the member it removed>`.
    let mut server = AofServer::start();
    let remaining;
    {
        let mut c = server.client();
        c.cmd(&["SADD", "aof_set", "a", "b", "c", "d", "e"]);
        c.cmd(&["SPOP", "aof_set"]);
        c.cmd(&["SPOP", "aof_set"]);
        remaining = c.cmd(&["SMEMBERS", "aof_set"]).sorted();
    }
    let aof = server.aof_contents();
    assert!(
        !aof.to_uppercase().contains("SPOP"),
        "SPOP must be rewritten before logging, got:\n{aof}"
    );

    server.restart();
    let mut c = server.client();
    assert_eq!(
        c.cmd(&["SMEMBERS", "aof_set"]).sorted(),
        remaining,
        "the restored set must match exactly, not just in size"
    );
}

#[test]
#[ignore = "Session 13: INCRBYFLOAT is logged as its result"]
fn incrbyfloat_is_logged_as_a_set() {
    // Adding 0.1 a hundred times does not land on exactly 10 in floating point,
    // and a replay can round differently. Log the result as `SET` so it can't
    // drift.
    let mut server = AofServer::start();
    let value;
    {
        let mut c = server.client();
        for _ in 0..100 {
            c.cmd(&["INCRBYFLOAT", "aof_f", "0.1"]);
        }
        value = c.cmd(&["GET", "aof_f"]).str().to_string();
    }
    server.restart();
    let mut c = server.client();
    assert_eq!(c.cmd(&["GET", "aof_f"]), bulk(&value));
}

// ---------------------------------------------------------------------------
// Transactions and damaged files
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 14 + 13: transactions are logged atomically"]
fn transactions_are_logged_as_multi_exec() {
    // Write MULTI/EXEC around the queued commands. Then a cut-off tail drops the
    // whole transaction instead of leaving `aof_t1` set and `aof_t2` missing.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        c.cmd(&["MULTI"]);
        c.cmd(&["SET", "aof_t1", "1"]);
        c.cmd(&["SET", "aof_t2", "2"]);
        c.cmd(&["EXEC"]);
    }
    let aof = server.aof_contents();
    assert!(aof.to_uppercase().contains("MULTI"));
    assert!(aof.to_uppercase().contains("EXEC"));

    server.restart();
    let mut c = server.client();
    assert_eq!(c.cmd(&["GET", "aof_t1"]), bulk("1"));
    assert_eq!(c.cmd(&["GET", "aof_t2"]), bulk("2"));
}

#[test]
#[ignore = "Session 13: a truncated AOF loads everything before the damage"]
fn a_truncated_tail_is_tolerated() {
    // A crash mid-write leaves the last command half-written -- here, 8 bytes
    // chopped off the end. Drop that fragment, load the 9 intact keys, start up.
    // Refusing to boot turns a power cut into an outage.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        for i in 0..10 {
            c.cmd(&["SET", &format!("aof_k{i}"), "v"]);
        }
    }
    server.stop();

    let mut bytes = std::fs::read(server.aof_path()).expect("no AOF to truncate");
    bytes.truncate(bytes.len() - 8);
    std::fs::write(server.aof_path(), bytes).unwrap();

    server.spawn();
    let mut c = server.client();
    assert_eq!(c.cmd(&["PING"]), simple("PONG"), "server must still start");
    let size = c.cmd(&["DBSIZE"]).int();
    assert!(
        (9..=10).contains(&size),
        "expected the intact prefix to load, got {size} keys"
    );
}

#[test]
#[ignore = "Session 13: an AOF with a bad command in the middle is refused"]
fn corruption_in_the_middle_is_not_silently_skipped() {
    // Not the same as a chopped tail: `!!!garbage!!!` spliced into the middle
    // means the file is not what you wrote. Refuse to start rather than serve a
    // keyspace that never existed.
    let mut server = AofServer::start();
    {
        let mut c = server.client();
        for i in 0..10 {
            c.cmd(&["SET", &format!("aof_k{i}"), "v"]);
        }
    }
    server.stop();

    let mut bytes = std::fs::read(server.aof_path()).unwrap();
    let midpoint = bytes.len() / 2;
    bytes.splice(midpoint..midpoint, b"!!!garbage!!!\r\n".iter().copied());
    std::fs::write(server.aof_path(), bytes).unwrap();

    // Spawn by hand, because `spawn()` waits for a port that must never open.
    let child = Command::new(env!("CARGO_BIN_EXE_mnemo"))
        .current_dir(&server.dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn");
    server.child = Some(child);
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        TcpStream::connect(("127.0.0.1", PORT)).is_err(),
        "the server should refuse to start on a corrupt AOF"
    );
}
