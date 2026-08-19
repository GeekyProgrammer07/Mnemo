//! Ported from `redis/tests/unit/multi.tcl`.
//!
//! MULTI/EXEC/DISCARD and WATCH. Two rules run through the whole file.
//!
//! 1. There is no rollback. MULTI queues, EXEC runs the queue back to back with
//!    nothing interleaved. If the fifth command fails the first four stay done.
//!
//! 2. When an error is caught decides what happens. Queue-time errors (unknown
//!    command, wrong arity) abort everything with EXECABORT. Run-time errors
//!    (WRONGTYPE) come back as one element of the EXEC array; the rest still run.
//!
//! Transaction state is per connection, so tests use a fresh `connect()`, or two
//! clients when the race is the point.
//!
//! Skipped from the original: replication propagation of transactions,
//! `DEBUG QUICKLIST-PACKED-THRESHOLD`, `RESET`, and the Lua interactions.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// Queueing
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 14: MULTI/EXEC"]
fn multi_queues_commands_and_exec_runs_them() {
    // Each queued command replies +QUEUED; the real replies come back together
    // as one array from EXEC, in queue order.
    let mut c = connect();
    c.del(&["mlt_a", "mlt_b"]);
    assert_eq!(c.cmd(&["MULTI"]), ok());
    assert_eq!(c.cmd(&["SET", "mlt_a", "1"]), simple("QUEUED"));
    assert_eq!(c.cmd(&["SET", "mlt_b", "2"]), simple("QUEUED"));
    assert_eq!(c.cmd(&["GET", "mlt_a"]), simple("QUEUED"));
    assert_eq!(
        c.cmd(&["EXEC"]),
        arr(vec![ok(), ok(), bulk("1")])
    );
    c.del(&["mlt_a", "mlt_b"]);
}

#[test]
#[ignore = "Session 14: queued commands must not run until EXEC"]
fn queued_commands_do_not_take_effect_before_exec() {
    // SET inside MULTI must not touch the data yet -- another client still sees
    // the key missing until EXEC. Running as you go and buffering replies passes
    // the test above and fails this one.
    let mut c = connect();
    let mut observer = connect();
    c.del(&["mlt_q"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["SET", "mlt_q", "written"]);
    assert_eq!(
        observer.cmd(&["EXISTS", "mlt_q"]),
        int(0),
        "the write must not be visible before EXEC"
    );
    c.cmd(&["EXEC"]);
    assert_eq!(observer.cmd(&["GET", "mlt_q"]), bulk("written"));
    c.del(&["mlt_q"]);
}

#[test]
#[ignore = "Session 14: empty MULTI/EXEC"]
fn exec_with_nothing_queued_returns_an_empty_array() {
    // Empty array, not nil -- nil is reserved for a WATCH abort.
    let mut c = connect();
    assert_eq!(c.cmd(&["MULTI"]), ok());
    assert_eq!(c.cmd(&["EXEC"]), arr(vec![]));
}

#[test]
#[ignore = "Session 14: MULTI cannot be nested"]
fn nested_multi_is_an_error() {
    // A second MULTI errors but does not abort: the first one is still open and
    // EXEC afterwards still works.
    let mut c = connect();
    assert_eq!(c.cmd(&["MULTI"]), ok());
    assert_error(&c.cmd(&["MULTI"]), "ERR MULTI calls can not be nested");
    assert_eq!(c.cmd(&["EXEC"]), arr(vec![]));
}

#[test]
#[ignore = "Session 14: EXEC/DISCARD without MULTI"]
fn exec_and_discard_without_multi_are_errors() {
    // Outside a transaction both are errors, not no-ops.
    let mut c = connect();
    assert_error(&c.cmd(&["EXEC"]), "ERR EXEC without MULTI");
    assert_error(&c.cmd(&["DISCARD"]), "ERR DISCARD without MULTI");
}

#[test]
#[ignore = "Session 14: DISCARD"]
fn discard_throws_away_the_queue() {
    // DISCARD drops the queued SET and leaves the connection out of MULTI.
    let mut c = connect();
    c.del(&["mlt_d"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["SET", "mlt_d", "value"]);
    assert_eq!(c.cmd(&["DISCARD"]), ok());
    assert_eq!(c.cmd(&["EXISTS", "mlt_d"]), int(0));
    // Back to normal, not still in a transaction.
    assert_eq!(c.cmd(&["PING"]), simple("PONG"));
    assert_error(&c.cmd(&["EXEC"]), "ERR EXEC without MULTI");
}

#[test]
#[ignore = "Session 14: transaction state is per connection"]
fn multi_state_does_not_leak_between_clients() {
    // Client a in MULTI leaves client b alone. Put the queue on the shared `Db`
    // instead of per-client state and this fails at once.
    let mut a = connect();
    let mut b = connect();
    a.cmd(&["MULTI"]);
    assert_eq!(b.cmd(&["PING"]), simple("PONG"), "b is not in a transaction");
    assert_error(&b.cmd(&["EXEC"]), "ERR EXEC without MULTI");
    a.cmd(&["DISCARD"]);
}

// ---------------------------------------------------------------------------
// Errors at queue time vs run time
//
// Queue time: the server can tell the command is broken just by looking at it
// (unknown name, wrong arity). It flags the transaction, and EXEC then replies
// EXECABORT and runs nothing at all.
//
// Run time: the command looked fine when queued and only failed against the
// actual data (WRONGTYPE, INCR on a string). Its error is one element of the
// EXEC array and the commands after it still run.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 14: a bad command at queue time aborts the transaction"]
fn queueing_an_unknown_command_aborts_the_transaction() {
    // The SET queued fine, but the unknown command poisons the transaction:
    // after EXECABORT, mlt_ab still does not exist.
    let mut c = connect();
    c.del(&["mlt_ab"]);
    c.cmd(&["MULTI"]);
    assert_eq!(c.cmd(&["SET", "mlt_ab", "1"]), simple("QUEUED"));
    assert_error(&c.cmd(&["NONEXISTINGCOMMAND"]), "ERR unknown command");
    assert_error(
        &c.cmd(&["EXEC"]),
        "EXECABORT Transaction discarded because of previous errors",
    );
    assert_eq!(c.cmd(&["EXISTS", "mlt_ab"]), int(0), "nothing ran");
}

#[test]
#[ignore = "Session 14: wrong arity at queue time aborts the transaction"]
fn queueing_a_command_with_wrong_arity_aborts_the_transaction() {
    // Same abort, from a bare `GET` with no key -- arity is checked at queue time.
    let mut c = connect();
    c.del(&["mlt_ar"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["SET", "mlt_ar", "1"]);
    assert_error(&c.cmd(&["GET"]), "ERR wrong number of arguments");
    assert_error(&c.cmd(&["EXEC"]), "EXECABORT");
    assert_eq!(c.cmd(&["EXISTS", "mlt_ar"]), int(0));
}

#[test]
#[ignore = "Session 14: runtime errors do not stop the transaction"]
fn a_runtime_error_appears_inside_the_exec_reply() {
    // "no rollback", made concrete: INCR on a string fails, and the SET queued
    // after it still runs. EXEC returns [error, +OK].
    let mut c = connect();
    c.del(&["mlt_str", "mlt_after"]);
    c.cmd(&["SET", "mlt_str", "notanumber"]);

    c.cmd(&["MULTI"]);
    assert_eq!(c.cmd(&["INCR", "mlt_str"]), simple("QUEUED"));
    assert_eq!(c.cmd(&["SET", "mlt_after", "ran"]), simple("QUEUED"));
    let replies = c.cmd(&["EXEC"]);
    let items = replies.array();
    assert_eq!(items.len(), 2);
    assert!(items[0].is_error(), "first command failed at runtime");
    assert_eq!(items[1], ok());
    assert_eq!(
        c.cmd(&["GET", "mlt_after"]),
        bulk("ran"),
        "later commands still run after an error"
    );
    c.del(&["mlt_str", "mlt_after"]);
}

#[test]
#[ignore = "Session 14: WRONGTYPE inside a transaction"]
fn wrongtype_inside_a_transaction_is_a_runtime_error() {
    // GET on a list queues fine and fails at EXEC; the PING behind it still runs.
    let mut c = connect();
    c.del(&["mlt_list"]);
    c.cmd(&["RPUSH", "mlt_list", "a"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["GET", "mlt_list"]);
    c.cmd(&["PING"]);
    let items = c.cmd(&["EXEC"]).array().to_vec();
    assert_wrongtype(&items[0]);
    assert_eq!(items[1], simple("PONG"));
    c.del(&["mlt_list"]);
}

// ---------------------------------------------------------------------------
// WATCH
//
// WATCH before MULTI, and if any watched key is written before EXEC the whole
// transaction is dropped. The abort signal is a null array (not an empty one --
// that is what an empty transaction returns), and it means "retry".
//
// The check is "was this key written", not "did the value change" and not "did
// someone else do it", so track a dirty flag per key rather than comparing.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 15: WATCH aborts EXEC when a watched key changes"]
fn watch_aborts_exec_if_the_key_was_touched() {
    // Another client writes mlt_w between WATCH and EXEC => EXEC returns nil and
    // the queued GET never runs; their write stands.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "1"]);

    assert_eq!(c.cmd(&["WATCH", "mlt_w"]), ok());
    other.cmd(&["SET", "mlt_w", "2"]);

    c.cmd(&["MULTI"]);
    c.cmd(&["GET", "mlt_w"]);
    assert_eq!(c.cmd(&["EXEC"]), Reply::NilArray);
    assert_eq!(c.cmd(&["GET", "mlt_w"]), bulk("2"), "the other write stands");
    c.del(&["mlt_w"]);
}

#[test]
#[ignore = "Session 15: WATCH lets EXEC through when nothing changed"]
fn watch_allows_exec_when_the_key_is_untouched() {
    // Nobody wrote mlt_w, so EXEC runs normally: [2] from the INCR.
    let mut c = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "1"]);
    c.cmd(&["WATCH", "mlt_w"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["INCR", "mlt_w"]);
    assert_eq!(c.cmd(&["EXEC"]), arr(vec![int(2)]));
    c.del(&["mlt_w"]);
}

#[test]
#[ignore = "Session 15: the watching client's own writes still count"]
fn watch_is_tripped_by_the_watching_clients_own_write() {
    // No exemption for the watcher: writing your own watched key aborts too.
    let mut c = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "1"]);
    c.cmd(&["WATCH", "mlt_w"]);
    c.cmd(&["SET", "mlt_w", "2"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["GET", "mlt_w"]);
    assert_eq!(c.cmd(&["EXEC"]), Reply::NilArray);
    c.del(&["mlt_w"]);
}

#[test]
#[ignore = "Session 15: a write of the same value still trips WATCH"]
fn watch_is_tripped_even_if_the_value_is_unchanged() {
    // SET mlt_w same over the same value still aborts -- any write counts.
    // Comparing old and new values instead of flagging writes fails here.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "same"]);
    c.cmd(&["WATCH", "mlt_w"]);
    other.cmd(&["SET", "mlt_w", "same"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["GET", "mlt_w"]);
    assert_eq!(c.cmd(&["EXEC"]), Reply::NilArray);
    c.del(&["mlt_w"]);
}

#[test]
#[ignore = "Session 15: WATCH on a key that gets deleted"]
fn watch_is_tripped_by_deletion() {
    // DEL counts as a write, so DEL must set the dirty flag too.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "1"]);
    c.cmd(&["WATCH", "mlt_w"]);
    other.cmd(&["DEL", "mlt_w"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["PING"]);
    assert_eq!(c.cmd(&["EXEC"]), Reply::NilArray);
}

#[test]
#[ignore = "Session 15: WATCH on a key that does not exist yet"]
fn watch_on_a_missing_key_is_tripped_by_its_creation() {
    // Watching a missing key is legal and trips when someone creates it, so
    // record the watch even when there is nothing there yet.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_new"]);
    c.cmd(&["WATCH", "mlt_new"]);
    other.cmd(&["SET", "mlt_new", "created"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["PING"]);
    assert_eq!(c.cmd(&["EXEC"]), Reply::NilArray);
    c.del(&["mlt_new"]);
}

#[test]
#[ignore = "Session 15: WATCH several keys"]
fn watch_several_keys_trips_on_any_of_them() {
    // WATCH takes many keys; a write to any one of them aborts.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_w1", "mlt_w2"]);
    c.cmd(&["MSET", "mlt_w1", "1", "mlt_w2", "2"]);
    c.cmd(&["WATCH", "mlt_w1", "mlt_w2"]);
    other.cmd(&["SET", "mlt_w2", "changed"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["PING"]);
    assert_eq!(c.cmd(&["EXEC"]), Reply::NilArray);
    c.del(&["mlt_w1", "mlt_w2"]);
}

#[test]
#[ignore = "Session 15: EXEC clears all watches"]
fn exec_clears_the_watch_list() {
    // EXEC clears the watch list either way. Leave a stale watch armed and the
    // next transaction on that connection aborts for no visible reason.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "1"]);
    c.cmd(&["WATCH", "mlt_w"]);
    c.cmd(&["MULTI"]);
    assert_eq!(c.cmd(&["EXEC"]), arr(vec![]));

    other.cmd(&["SET", "mlt_w", "2"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["PING"]);
    assert_eq!(
        c.cmd(&["EXEC"]),
        arr(vec![simple("PONG")]),
        "the old watch must not still be armed"
    );
    c.del(&["mlt_w"]);
}

#[test]
#[ignore = "Session 15: DISCARD clears all watches"]
fn discard_clears_the_watch_list() {
    // DISCARD clears watches too, not just the queue.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "1"]);
    c.cmd(&["WATCH", "mlt_w"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["DISCARD"]);

    other.cmd(&["SET", "mlt_w", "2"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["PING"]);
    assert_eq!(c.cmd(&["EXEC"]), arr(vec![simple("PONG")]));
    c.del(&["mlt_w"]);
}

#[test]
#[ignore = "Session 15: UNWATCH"]
fn unwatch_cancels_the_watches() {
    // UNWATCH drops every watch, so the later write cannot abort anything.
    let mut c = connect();
    let mut other = connect();
    c.del(&["mlt_w"]);
    c.cmd(&["SET", "mlt_w", "1"]);
    c.cmd(&["WATCH", "mlt_w"]);
    assert_eq!(c.cmd(&["UNWATCH"]), ok());
    other.cmd(&["SET", "mlt_w", "2"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["PING"]);
    assert_eq!(c.cmd(&["EXEC"]), arr(vec![simple("PONG")]));
    c.del(&["mlt_w"]);
}

#[test]
#[ignore = "Session 15: WATCH inside MULTI is an error"]
fn watch_inside_multi_is_an_error() {
    // Too late to watch once queueing has started, so it is an error, not a
    // silent no-op. Note it does not abort -- DISCARD still works.
    let mut c = connect();
    c.cmd(&["MULTI"]);
    assert_error(&c.cmd(&["WATCH", "mlt_w"]), "ERR WATCH inside MULTI is not allowed");
    c.cmd(&["DISCARD"]);
}

#[test]
#[ignore = "Session 15: WATCH is tripped by expiry"]
fn watch_is_tripped_when_a_watched_key_expires() {
    // A key expiring on its own counts as a change. Nobody wrote it, so the
    // dirty flag has to be set by whatever drops it -- lazy and active sweep both.
    let mut c = connect();
    c.del(&["mlt_exp"]);
    c.cmd(&["SET", "mlt_exp", "1", "PX", "100"]);
    c.cmd(&["WATCH", "mlt_exp"]);
    std::thread::sleep(std::time::Duration::from_millis(300));
    c.cmd(&["MULTI"]);
    c.cmd(&["PING"]);
    assert_eq!(c.cmd(&["EXEC"]), Reply::NilArray);
}

// ---------------------------------------------------------------------------
// Isolation
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Session 14: EXEC runs without interleaving"]
fn exec_is_isolated_from_other_clients() {
    // Two clients each queue 20 INCRs on one counter. If EXEC does not
    // interleave, each client's 20 replies are consecutive (1..20 and 21..40)
    // and the counter ends at exactly 40.
    let mut a = connect();
    let mut b = connect();
    a.del(&["mlt_iso"]);
    a.cmd(&["SET", "mlt_iso", "0"]);

    a.cmd(&["MULTI"]);
    for _ in 0..20 {
        a.cmd(&["INCR", "mlt_iso"]);
    }
    b.cmd(&["MULTI"]);
    for _ in 0..20 {
        b.cmd(&["INCR", "mlt_iso"]);
    }

    let a_replies = a.cmd(&["EXEC"]).array().to_vec();
    let b_replies = b.cmd(&["EXEC"]).array().to_vec();

    // Each transaction's replies must be a contiguous run of 20 values.
    for replies in [&a_replies, &b_replies] {
        let first = replies[0].int();
        for (i, reply) in replies.iter().enumerate() {
            assert_eq!(
                reply.int(),
                first + i as i64,
                "another client interleaved inside EXEC"
            );
        }
    }
    assert_eq!(a.cmd(&["GET", "mlt_iso"]), bulk("40"));
    a.del(&["mlt_iso"]);
}

#[test]
#[ignore = "Session 14: MULTI followed by a disconnect"]
fn a_disconnect_during_multi_discards_the_queue() {
    // Dropping the socket with a SET queued must not run it -- per-connection
    // state has to die with the connection.
    let mut c = connect();
    c.del(&["mlt_drop"]);
    c.cmd(&["MULTI"]);
    c.cmd(&["SET", "mlt_drop", "written"]);
    drop(c);

    let mut check = connect();
    assert_eq!(check.cmd(&["EXISTS", "mlt_drop"]), int(0));
}
