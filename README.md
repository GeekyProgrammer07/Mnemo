# Mnemo

A Redis server written from scratch in Rust — TCP, the RESP protocol, and the
commands, all by hand. No `redis` crate, no parser library.

This is a learning project, not a product. The point is understanding how a real
database server works, so the code is commented to explain *why* rather than
*what*.

## Running it

```bash
cargo run
```

Listens on `127.0.0.1:6379`, the standard Redis port, so any Redis client works:

```bash
redis-cli -p 6379 PING
```

No `redis-cli` installed? just get docker

```bash
docker run --network host -it redis:latest redis-cli -p 6379
```

Or send raw RESP with nothing in between:

```bash
printf '*1\r\n$4\r\nPING\r\n' | nc 127.0.0.1 6379
```

Inline commands work too — `printf 'PING\r\n' | nc 127.0.0.1 6379`.

## Testing

```bash
cargo test              # everything
cargo test --bin mnemo  # unit tests only, no server needed
```

The integration tests start the server themselves. If results look stale, a
server from an earlier run is still holding the port. Either `pkill mnemo`, or
find it with `lsof -i :6379` and `kill <PID>`.

**Current state: 89 passing, 11 failing, 262 switched off.** The 262 are ported
from Redis's own TCL suite and turned off with `#[ignore]` until the feature
exists — they are the roadmap, not a problem. See `tests/`.

## Where it is

TCP server, RESP2 parse/encode, inline commands, `PING` `ECHO` `SET` `GET`
`DEL` `EXISTS` `TYPE` `MSET` `MGET`.

All of them work in `redis-cli` exactly as they do against real Redis:

```
127.0.0.1:6379> PING
PONG
127.0.0.1:6379> SET name suman
OK
127.0.0.1:6379> GET name
"suman"
127.0.0.1:6379> MSET a 1 b 2
OK
127.0.0.1:6379> MGET a nope b
1) "1"
2) (nil)
3) "2"
127.0.0.1:6379> TYPE name
string
127.0.0.1:6379> EXISTS name nope
(integer) 1
127.0.0.1:6379> DEL name
(integer) 1
127.0.0.1:6379> GET name
(nil)
```

## Docs

```bash
cargo doc --document-private-items --open
```

An overview of every function. `--document-private-items` matters here — most of
this codebase is private, so plain `cargo doc` shows almost nothing.

The doc comments are mostly AI-written, but I've read and reviewed all of them : )
