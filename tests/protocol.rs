//! Ported from `redis/tests/unit/protocol.tcl`.
//!
//! The RESP wire format itself: what the server does with malformed frames,
//! oversized headers, and garbage bytes.
//!
//! Skipped from the original: everything behind `hello 3` (RESP3) and everything
//! using `DEBUG PROTOCOL`. Scope here is RESP2.
//!
//! Running theme: on a protocol error Redis replies `-ERR Protocol error: ...`
//! and only THEN closes. Closing silently is why a client reports "connection
//! reset" instead of something useful.

mod common;
use common::*;

// ---------------------------------------------------------------------------
// Malformed frames
//
// Each of these sends bad bytes and expects `-ERR Protocol error: ...` back
// before the connection goes away.
// ---------------------------------------------------------------------------

#[test]
fn handle_an_empty_query() {
    // A bare `\r\n` is nothing at all. Skip it and keep reading -- don't wait
    // forever for the rest of a frame that isn't coming.
    let mut c = connect();
    c.write_raw(b"\r\n");
    assert_eq!(c.cmd(&["PING"]), simple("PONG"));
}

#[test]
fn negative_multibulk_length() {
    // `*-1` is the null array: legal RESP, not a legal command. Ignore it,
    // don't error.
    let mut c = connect();
    c.write_raw(b"*-10\r\n");
    assert_eq!(c.cmd(&["PING"]), simple("PONG"));
}

#[test]
fn out_of_range_multibulk_length() {
    // `*3000000000\r\n` would allocate a 3-billion-entry Vec before any argument
    // arrives -- a one-packet OOM. Redis caps it at 1024*1024.
    let mut c = connect();
    c.write_raw(b"*3000000000\r\n");
    let reply = c.read_reply();
    assert_error(&reply, "ERR Protocol error");
    assert!(reply.error().contains("invalid multibulk length"));
}

#[test]
fn wrong_multibulk_payload_header() {
    // Every element of a command array must start with '$'. Here the 3rd is
    // `fooz\r\n` -- the stream is out of sync, so stop instead of reading
    // payload bytes as commands.
    let mut c = connect();
    c.write_raw(b"*3\r\n$3\r\nSET\r\n$1\r\nx\r\nfooz\r\n");
    let reply = c.read_reply();
    assert_error(&reply, "ERR Protocol error");
    assert!(reply.error().contains("expected '$', got 'f'"));
}

#[test]
fn negative_multibulk_payload_length() {
    // `$-1` is the null bulk -- fine in a reply, never a command argument.
    let mut c = connect();
    c.write_raw(b"*3\r\n$3\r\nSET\r\n$1\r\nx\r\n$-10\r\n");
    let reply = c.read_reply();
    assert_error(&reply, "ERR Protocol error");
    assert!(reply.error().contains("invalid bulk length"));
}

#[test]
fn out_of_range_multibulk_payload_length() {
    // Same allocation trap one level down: `$2000000000` asks for 2GB. Redis
    // caps a single argument at 512MB.
    let mut c = connect();
    c.write_raw(b"*3\r\n$3\r\nSET\r\n$1\r\nx\r\n$2000000000\r\n");
    let reply = c.read_reply();
    assert_error(&reply, "ERR Protocol error");
    assert!(reply.error().contains("invalid bulk length"));
}

#[test]
fn non_number_multibulk_payload_length() {
    // `$blabla` is not a length. Reject it -- don't let a failed parse fall
    // through as 0.
    let mut c = connect();
    c.write_raw(b"*3\r\n$3\r\nSET\r\n$1\r\nx\r\n$blabla\r\n");
    let reply = c.read_reply();
    assert_error(&reply, "ERR Protocol error");
    assert!(reply.error().contains("invalid bulk length"));
}

#[test]
fn multi_bulk_request_not_followed_by_bulk_arguments() {
    // `*1\r\nfoo\r\n`: the header promised one `$` element and got bare text.
    let mut c = connect();
    c.write_raw(b"*1\r\nfoo\r\n");
    let reply = c.read_reply();
    assert_error(&reply, "ERR Protocol error");
    assert!(reply.error().contains("expected '$', got 'f'"));
}

// ---------------------------------------------------------------------------
// Inline commands
//
// A line that does not start with '*' is a plain-text command. That is what
// makes `telnet localhost 6379` then typing `PING` work.
// ---------------------------------------------------------------------------

#[test]
fn inline_command_works() {
    // `PING\r\n` => `+PONG\r\n`, no RESP array involved.
    let mut c = connect();
    c.write_raw(b"PING\r\n");
    assert_eq!(c.read_reply(), simple("PONG"));
}

#[test]
fn unbalanced_number_of_quotes() {
    // Inline lines use shell-style quoting, so they can be malformed in a way
    // RESP arrays cannot: `set """test-key""" test-value` never closes its quote.
    let mut c = connect();
    c.write_raw(b"set \"\"\"test-key\"\"\" test-value\r\n");
    c.write_raw(b"ping\r\n");
    let reply = c.read_reply();
    assert_error(&reply, "ERR Protocol error");
    assert!(reply.error().contains("unbalanced"));
}

// ---------------------------------------------------------------------------
// Desync recovery
// ---------------------------------------------------------------------------

/// One client sends `\x00`, `*\x00` or `$\x00` and then floods 'A's forever,
/// never finishing the frame. The server must stop buffering and answer with a
/// protocol error instead of growing memory until it dies.
#[test]
fn protocol_desync_regression() {
    for seq in [&b"\x00"[..], &b"*\x00"[..], &b"$\x00"[..]] {
        let mut c = connect();
        c.write_raw(seq);

        let payload = vec![b'A'; 1024];
        let mut wrote = 0usize;
        // Stop at 64MB so a server that never pushes back fails instead of
        // hanging the suite.
        while wrote < 64 * 1024 * 1024 {
            c.write_raw(&payload);
            wrote += payload.len();
        }

        let reply = c.read_reply();
        assert!(
            reply.error().contains("Protocol error"),
            "expected a protocol error after {wrote} bytes of desync, got {reply:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Well-formed traffic
// ---------------------------------------------------------------------------

#[test]
fn generic_wrong_number_of_args() {
    // Check arg count before the handler runs. `PING x y z` => `-ERR wrong
    // number of arguments for 'ping'`, and the handler never sees it.
    let mut c = connect();
    let reply = c.cmd(&["PING", "x", "y", "z"]);
    assert_error(&reply, "ERR wrong number of arguments");
    assert!(reply.error().contains("ping"));
}

#[test]
#[ignore = "Session 4: in-memory store (GET/SET)"]
fn bulk_reply_protocol_exact_bytes() {
    // A value goes out the same bytes it came in, whatever its size or shape.
    // `SET crlf 2` then `GET crlf` => `$1\r\n2\r\n`.
    let mut c = connect();
    for value in [
        "2",
        "2147483647",
        "-2147483648",
        "-9223372036854775809",
        "9223372036854775808",
        "aaaaaaaaaaaaaaaa",
        &"a".repeat(45),
    ] {
        assert_eq!(c.cmd(&["SET", "crlf", value]), ok());
        assert_eq!(c.cmd(&["GET", "crlf"]), bulk(value));
    }
    assert_eq!(c.cmd(&["DEL", "crlf"]), int(1));
}

#[test]
#[ignore = "Session 4: in-memory store (GET/SET)"]
fn empty_bulk_string_round_trips() {
    // Empty string `$0\r\n\r\n` is a real value; nil is `$-1\r\n`. Mixing up the
    // two is the classic hand-rolled-RESP bug.
    let mut c = connect();
    c.del(&["proto_empty"]);
    assert_eq!(c.cmd(&["SET", "proto_empty", ""]), ok());
    assert_eq!(c.cmd(&["GET", "proto_empty"]), bulk(""));
    assert!(!c.cmd(&["GET", "proto_empty"]).is_nil());
    assert!(c.cmd(&["GET", "proto_missing"]).is_nil());
}

#[test]
#[ignore = "Session 4: in-memory store (GET/SET)"]
fn binary_safe_values() {
    // Keys and values are bytes, not text -- NUL, `\xff` and an embedded `\r\n`
    // all round-trip. Any `String::from_utf8` in the path breaks here.
    let mut c = connect();
    let value: &[u8] = b"\x00\x01\x02\xff\xfe\r\n\x00binary";
    assert_eq!(c.cmd::<&[u8]>(&[b"SET", b"proto_bin", value]), ok());
    assert_eq!(
        c.cmd::<&[u8]>(&[b"GET", b"proto_bin"]),
        Reply::Bulk(value.to_vec())
    );
}

#[test]
fn commands_pipelining() {
    // Three commands in one write get three replies in order, so one `read()`
    // must drain every complete frame in the buffer, not just the first.
    let mut c = connect();
    c.send(&["SET", "pipe_k1", "xyzk"]);
    c.send(&["GET", "pipe_k1"]);
    c.send(&["PING"]);
    assert_eq!(c.read_reply(), ok());
    assert_eq!(c.read_reply(), bulk("xyzk"));
    assert_eq!(c.read_reply(), simple("PONG"));
}

#[test]
fn command_split_across_packets() {
    // TCP has no message boundaries: `SET split_k` arrives, then a pause, then
    // the rest. Say "incomplete", keep the bytes, resume when more arrive.
    let mut c = connect();
    c.write_raw(b"*3\r\n$3\r\nSET\r\n$7\r\nsplit_k");
    std::thread::sleep(std::time::Duration::from_millis(50));
    c.write_raw(b"\r\n$5\r\nvalue\r\n");
    assert_eq!(c.read_reply(), ok());
    assert_eq!(c.cmd(&["GET", "split_k"]), bulk("value"));
}

#[test]
fn non_existing_command() {
    // `-ERR unknown command` is an answer, not a hangup: the next `PING` on the
    // same connection must still work.
    let mut c = connect();
    assert_error(&c.cmd(&["foobaredcommand"]), "ERR unknown command");
    assert_eq!(c.cmd(&["PING"]), simple("PONG"));
}

#[test]
fn command_names_are_case_insensitive() {
    // `ping`, `PiNg`, `PING` are the same command. Lowercase the name before
    // you look it up.
    let mut c = connect();
    assert_eq!(c.cmd(&["ping"]), simple("PONG"));
    assert_eq!(c.cmd(&["PiNg"]), simple("PONG"));
}

#[test]
#[ignore = "Session 5: string commands (MSET/MGET)"]
fn test_large_number_of_args() {
    // One MSET with 20,000 arguments. Catches a parser that recurses per
    // argument (stack overflow) or reallocates per argument (crawls).
    let mut c = connect();
    let mut args: Vec<String> = vec!["MSET".into()];
    for i in 0..10_000 {
        args.push(format!("bigargs_k{i}"));
        args.push("v".into());
    }
    args.push("bigargs_k2".into());
    args.push("v2".into());
    assert_eq!(c.cmd(&args), ok());
    assert_eq!(c.cmd(&["GET", "bigargs_k2"]), bulk("v2"));
}

#[test]
fn large_bulk_payload() {
    // A 10MB bulk string cannot arrive in one read, so the "need more data"
    // path runs hundreds of times for a single command.
    let mut c = connect();
    let value = "x".repeat(10 * 1024 * 1024);
    assert_eq!(c.cmd(&["SET", "proto_big", &value]), ok());
    assert_eq!(c.cmd(&["STRLEN", "proto_big"]), int(value.len() as i64));
    assert_eq!(c.cmd(&["GET", "proto_big"]), bulk(&value));
    c.del(&["proto_big"]);
}

#[test]
fn multiple_clients_are_independent() {
    // Two connections, one keyspace: `a` writes `shared_key`, `b` reads it. The
    // data is shared, the per-connection state is not -- MULTI relies on this.
    let mut a = connect();
    let mut b = connect();
    assert_eq!(a.cmd(&["SET", "shared_key", "from_a"]), ok());
    assert_eq!(b.cmd(&["GET", "shared_key"]), bulk("from_a"));
    assert_eq!(b.cmd(&["PING"]), simple("PONG"));
    assert_eq!(a.cmd(&["PING"]), simple("PONG"));
}
