//! Test harness for the Mnemo integration suite.
//!
//! The Rust equivalent of what `tests/support/server.tcl` and `util.tcl` do in
//! the real Redis repo: get a server running, hand you a client speaking RESP
//! over a real socket, and give you assertion helpers.
//!
//! Three choices worth understanding:
//!
//! 1. **We spawn the actual binary and talk to it over TCP.** Redis tests the
//!    server the way a client sees it, because most real bugs live in the
//!    protocol and the connection state machine, not inside a single function.
//!    A socket test catches "you replied `+OK` where Redis replies `:1`" -- a
//!    whole class of bug that calling `dispatch()` directly would never surface.
//!
//! 2. **Blocking `std::net::TcpStream`, not tokio.** The *server* must be async
//!    to serve many clients; a test client talks to one server and waits for one
//!    reply, so async buys nothing and costs readability. Tests stay plain
//!    `#[test]` functions.
//!
//! 3. **One shared server on 6379, reused rather than restarted.** Your server
//!    hardcodes its port, so the harness cannot ask for a free one. Instead it
//!    connects to 6379 if something is already listening and spawns the binary
//!    only if nothing is. That makes the suite safe to run repeatedly and safe
//!    under `cargo test`'s parallel test binaries.
//!
//!    The cost: **tests share one keyspace**, so every test must use key names
//!    nobody else uses. The ported tests below follow that rule. If you later
//!    give the server a `--port` flag, this harness can spawn an isolated server
//!    per test binary and the constraint disappears.
//!
//! Leftover server: the spawned process outlives the test run by design (the
//! next run reuses it). `pkill mnemo` if you want it gone.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Where the server under test listens. Matches the hardcoded address in
/// `src/main.rs`.
pub const PORT: u16 = 6379;

// ---------------------------------------------------------------------------
// Replies
// ---------------------------------------------------------------------------

/// A decoded RESP2 reply.
///
/// This deliberately mirrors the *wire* types rather than reusing your internal
/// `RespType`. If you refactor `resp::parser`, these tests should still compile
/// and still assert the same bytes. A test that shares types with the code under
/// test can be broken by a rename -- or worse, pass through a real bug because
/// both sides changed together.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Simple(String),
    Error(String),
    Int(i64),
    Bulk(Vec<u8>),
    /// `$-1\r\n` -- the null bulk string.
    Nil,
    Array(Vec<Reply>),
    /// `*-1\r\n` -- the null array.
    NilArray,
}

impl Reply {
    /// Bulk or simple string as UTF-8. Panics on any other type, which is what
    /// you want in a test -- a wrong type is a failure, not something to handle.
    #[track_caller]
    pub fn str(&self) -> &str {
        match self {
            Reply::Bulk(b) => std::str::from_utf8(b).expect("reply was not utf-8"),
            Reply::Simple(s) => s,
            other => panic!("expected a string reply, got {other:?}"),
        }
    }

    #[track_caller]
    pub fn int(&self) -> i64 {
        match self {
            Reply::Int(n) => *n,
            other => panic!("expected an integer reply, got {other:?}"),
        }
    }

    #[track_caller]
    pub fn array(&self) -> &[Reply] {
        match self {
            Reply::Array(items) => items,
            other => panic!("expected an array reply, got {other:?}"),
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Reply::Nil | Reply::NilArray)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Reply::Error(_))
    }

    #[track_caller]
    pub fn error(&self) -> &str {
        match self {
            Reply::Error(e) => e,
            other => panic!("expected an error reply, got {other:?}"),
        }
    }

    /// Bulk strings of an array reply. Handy for `LRANGE`, `HKEYS`, `SMEMBERS`.
    #[track_caller]
    pub fn strings(&self) -> Vec<String> {
        self.array().iter().map(|r| r.str().to_string()).collect()
    }

    /// Like `strings()` but sorted -- the TCL suite writes `lsort [r smembers x]`
    /// constantly, because sets and hashes have no defined iteration order.
    #[track_caller]
    pub fn sorted(&self) -> Vec<String> {
        let mut v = self.strings();
        v.sort();
        v
    }
}

// Constructors, so assertions read close to the TCL originals.

pub fn ok() -> Reply {
    Reply::Simple("OK".into())
}
pub fn simple(s: &str) -> Reply {
    Reply::Simple(s.into())
}
pub fn int(n: i64) -> Reply {
    Reply::Int(n)
}
pub fn bulk(s: &str) -> Reply {
    Reply::Bulk(s.as_bytes().to_vec())
}
pub fn nil() -> Reply {
    Reply::Nil
}
pub fn arr(items: Vec<Reply>) -> Reply {
    Reply::Array(items)
}
/// An array of bulk strings -- by far the most common expected shape.
pub fn bulks(items: &[&str]) -> Reply {
    Reply::Array(items.iter().map(|s| bulk(s)).collect())
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// Make sure a server is listening on [`PORT`], starting one if needed.
///
/// Idempotent and safe to call from every test: the `OnceLock` collapses the
/// many calls inside one test binary into a single check, and the
/// already-listening branch handles the several test binaries `cargo test` runs
/// at once.
pub fn ensure_server() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        if TcpStream::connect(("127.0.0.1", PORT)).is_ok() {
            return; // something is already serving -- reuse it
        }

        Command::new(env!("CARGO_BIN_EXE_mnemo"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn the mnemo binary");

        // Poll until it accepts connections rather than sleeping a fixed amount.
        // A fixed sleep is flaky on a loaded machine and wastes time on an idle
        // one; polling is correct on both.
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", PORT)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("server did not start listening on port {PORT} within 10s");
    });
}

/// Connect a fresh client, starting the server if this is the first call.
///
/// Every test begins with `let mut c = connect();`.
pub fn connect() -> Client {
    ensure_server();
    Client::connect(PORT)
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct Client {
    stream: BufReader<TcpStream>,
}

impl Client {
    pub fn connect(port: u16) -> Client {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("could not connect to server");
        stream.set_nodelay(true).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        Client {
            stream: BufReader::new(stream),
        }
    }

    /// Send one command and read one reply.
    ///
    /// Args are `AsRef<[u8]>` so tests can send non-UTF-8 payloads. Redis keys
    /// and values are binary safe, and some tests below rely on it.
    #[track_caller]
    pub fn cmd<T: AsRef<[u8]>>(&mut self, args: &[T]) -> Reply {
        self.send(args);
        self.read_reply()
    }

    /// Send without reading -- for pipelining and `MULTI` queueing tests.
    pub fn send<T: AsRef<[u8]>>(&mut self, args: &[T]) {
        let mut out = format!("*{}\r\n", args.len()).into_bytes();
        for arg in args {
            let a = arg.as_ref();
            out.extend_from_slice(format!("${}\r\n", a.len()).as_bytes());
            out.extend_from_slice(a);
            out.extend_from_slice(b"\r\n");
        }
        self.write_raw(&out);
    }

    /// Write bytes verbatim. Protocol tests need to send malformed frames that
    /// `send()` could never produce.
    pub fn write_raw(&mut self, bytes: &[u8]) {
        self.stream
            .get_mut()
            .write_all(bytes)
            .expect("failed to write to server");
    }

    #[track_caller]
    pub fn read_reply(&mut self) -> Reply {
        let line = self.read_line();
        let (kind, body) = line.split_at(1);
        match kind {
            "+" => Reply::Simple(body.to_string()),
            "-" => Reply::Error(body.to_string()),
            ":" => Reply::Int(body.parse().expect("bad integer reply")),
            "$" => {
                let len: i64 = body.parse().expect("bad bulk length");
                if len < 0 {
                    return Reply::Nil;
                }
                let mut buf = vec![0u8; len as usize + 2]; // payload + CRLF
                self.stream
                    .read_exact(&mut buf)
                    .expect("short read on bulk payload");
                buf.truncate(len as usize);
                Reply::Bulk(buf)
            }
            "*" => {
                let count: i64 = body.parse().expect("bad array length");
                if count < 0 {
                    return Reply::NilArray;
                }
                Reply::Array((0..count).map(|_| self.read_reply()).collect())
            }
            other => panic!("unknown RESP type byte {other:?} in line {line:?}"),
        }
    }

    #[track_caller]
    fn read_line(&mut self) -> String {
        let mut line = String::new();
        let n = self
            .stream
            .read_line(&mut line)
            .expect("failed to read from server");
        if n == 0 {
            panic!("server closed the connection while we were waiting for a reply");
        }
        line.trim_end_matches(['\r', '\n']).to_string()
    }

    // -- shorthands used constantly in the ported tests -------------------

    /// Delete keys, ignoring the reply. The TCL suite opens most tests with
    /// `r del mykey` to clear state left by an earlier test.
    pub fn del(&mut self, keys: &[&str]) {
        let mut args = vec!["DEL"];
        args.extend_from_slice(keys);
        self.cmd(&args);
    }
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/// Assert an error reply whose message starts with `prefix`.
///
/// Redis error strings are checked by prefix (`ERR*`, `WRONGTYPE*`) in the TCL
/// suite, never compared whole. Matching exact wording would pin you to Redis's
/// phrasing, which changes between versions; the prefix is the part real clients
/// actually branch on.
#[track_caller]
pub fn assert_error(reply: &Reply, prefix: &str) {
    match reply {
        Reply::Error(msg) => assert!(
            msg.starts_with(prefix),
            "expected an error starting with {prefix:?}, got {msg:?}"
        ),
        other => panic!("expected an error starting with {prefix:?}, got {other:?}"),
    }
}

/// `WRONGTYPE Operation against a key holding the wrong kind of value`
#[track_caller]
pub fn assert_wrongtype(reply: &Reply) {
    assert_error(reply, "WRONGTYPE");
}

/// Assert a float reply is within a small epsilon. Float replies arrive as bulk
/// strings, and exact string comparison on floats is a losing game -- this is
/// what `roundFloat` does throughout the TCL suite.
#[track_caller]
pub fn assert_float(reply: &Reply, expected: f64) {
    let got: f64 = reply.str().parse().expect("reply was not a number");
    assert!(
        (got - expected).abs() < 0.001,
        "expected roughly {expected}, got {got}"
    );
}
