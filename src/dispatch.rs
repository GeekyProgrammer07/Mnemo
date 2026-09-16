use crate::{
    resp::parser::RespTypes::{self, BulkString},
    store::{
        db::Db,
        value::RedisValue,
    },
};

/// Runs one command and returns the reply to send back.
///
/// A command is always an array: element 0 is the name, the rest are the
/// arguments. Anything else gets an `-ERR` reply, not a dropped connection.
///
/// # Example
///
/// ```text
/// ["PING"]          -> +PONG
/// ["GET", "nope"]   -> $-1   (nil)
/// ["BLAH"]          -> -ERR unknown command 'BLAH'
/// ```
pub fn dispatch(frame: RespTypes, store: &mut Db) -> RespTypes {
    let parts = match frame {
        RespTypes::Array(Some(parts)) if !parts.is_empty() => parts,
        RespTypes::Array(_) => return error("ERR empty command"),
        _ => return error("ERR expected an array of bulk strings"),
    };

    // Kept as raw bytes, not `String`: a value may be a JPEG or could be anything.
    // Only the command name has to be text
    let mut args: Vec<Vec<u8>> = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            RespTypes::BulkString(Some(bytes)) => args.push(bytes),
            _ => return error("ERR every element must be a bulk string"),
        }
    }

    // Command names are case-insensitive: `ping` and `PING` are the same command.
    // `remove(0)` takes the name out, so element 0 is now the first argument.
    let name = match String::from_utf8(args.remove(0)) {
        Ok(name) => name.to_uppercase(),
        Err(_) => return error("ERR command name must be valid UTF-8"),
    };

    match name.as_str() {
        "PING" => cmd_ping(args),
        "ECHO" => cmd_echo(args),
        "SET" => cmd_set(args, store),
        "GET" => cmd_get(&args, store),
        "DEL" => cmd_del(&args, store),
        "EXISTS" => cmd_exists(&args, store),
        "TYPE" => cmd_type(&args, store),
        "MSET" => cmd_mset(args, store),
        "MGET" => cmd_mget(&args, store),
        other => error(&format!("ERR unknown command '{other}'")),
    }
}

/// `PING` — the "are you alive?" command.
///
/// # Example
///
/// ```text
/// PING        -> +PONG
/// PING hello  -> $5\r\nhello   (echoes the argument instead)
/// PING a b    -> -ERR wrong number of arguments for 'ping' command
/// ```
fn cmd_ping(mut args: Vec<Vec<u8>>) -> RespTypes {
    match args.len() {
        0 => RespTypes::SimpleString("PONG".to_string()),
        // `PING hello` replies `hello` instead of `PONG`.
        1 => RespTypes::BulkString(Some(args.remove(0))),
        _ => wrong_arity("ping"),
    }
}

/// `ECHO message` — sends the same message straight back.
///
/// # Example
///
/// ```text
/// ECHO hello  -> $5\r\nhello
/// ECHO        -> -ERR wrong number of arguments for 'echo' command
/// ```
fn cmd_echo(mut args: Vec<Vec<u8>>) -> RespTypes {
    match args.len() {
        1 => RespTypes::BulkString(Some(args.remove(0))),
        _ => wrong_arity("echo"),
    }
}

/// `SET key value` — stores bytes under a key. Overwriting is fine.
///
/// # Example
///
/// ```text
/// SET foo bar  -> +OK
/// SET foo baz  -> +OK   (replaces the old value)
/// SET foo      -> -ERR wrong number of arguments for 'set' command
/// ```
fn cmd_set(mut args: Vec<Vec<u8>>, store: &mut Db) -> RespTypes {
    if args.len() != 2 {
        return wrong_arity("set");
    }
    let value = args.remove(1);
    let key = match key_of(&args[0]) {
        Ok(key) => key,
        Err(reply) => return reply,
    };
    // `set` hands back whatever was there before. Overwriting is normal in
    // Redis — `SET k v` twice replies `OK` both times — so the old value is
    // dropped rather than reported.
    store.set(key, RedisValue::String(value));
    RespTypes::SimpleString("OK".to_string())
}

/// `GET key` — the stored bytes, or nil if the key isn't there.
///
/// # Example
///
/// ```text
/// GET foo   -> $3\r\nbar
/// GET nope  -> $-1        (redis-cli shows this as "(nil)")
/// ```
fn cmd_get(args: &[Vec<u8>], store: &Db) -> RespTypes {
    if args.len() != 1 {
        return wrong_arity("get");
    }
    let key = match key_of(&args[0]) {
        Ok(key) => key,
        Err(reply) => return reply,
    };
    match store.get(&key) {
        // A missing key is the *null* bulk string, which `redis-cli` prints as
        // `(nil)`. Replying with a normal string would look like a stored value.
        None => RespTypes::BulkString(None),
        Some(RedisValue::String(bytes)) => RespTypes::BulkString(Some(bytes.clone())),
    }
}

/// `DEL key [key ...]` — deletes keys, replies how many were actually removed.
///
/// Deleting a key that isn't there is not an error, it just doesn't count.
///
/// # Example
///
/// ```text
/// DEL foo         -> :1
/// DEL foo         -> :0   (already gone)
/// DEL a b nothing -> :2
/// ```
fn cmd_del(args: &[Vec<u8>], store: &mut Db) -> RespTypes {
    if args.is_empty() {
        return wrong_arity("del");
    }
    // DEL takes any number of keys and replies with how many it actually removed.
    let mut removed = 0;
    for arg in args {
        if let Ok(key) = key_of(arg) {
            if store.del(&key).is_some() {
                removed += 1;
            }
        }
    }
    RespTypes::Integer(removed)
}

/// `EXISTS key [key ...]` — how many of these keys exist.
///
/// # Example
///
/// ```text
/// EXISTS foo      -> :1
/// EXISTS foo foo  -> :2   (repeats counted twice, not de-duplicated)
/// EXISTS nope     -> :0
/// ```
fn cmd_exists(args: &[Vec<u8>], store: &Db) -> RespTypes {
    if args.is_empty() {
        return wrong_arity("exists");
    }
    // Counts repeats: `EXISTS k k` on one stored key replies 2.
    let mut found = 0;
    for arg in args {
        if let Ok(key) = key_of(arg) {
            if store.exists(&key) {
                found += 1;
            }
        }
    }
    RespTypes::Integer(found)
}

/// `TYPE key` — the type name of the stored value.
///
/// # Example
///
/// ```text
/// TYPE foo   -> +string
/// TYPE nope  -> +none     (not an error, not nil)
/// ```
fn cmd_type(args: &[Vec<u8>], store: &Db) -> RespTypes {
    if args.len() != 1 {
        return wrong_arity("type");
    }
    let key = match key_of(&args[0]) {
        Ok(key) => key,
        Err(reply) => return reply,
    };
    // A missing key is the literal string `none`, not an error and not a nil.
    RespTypes::SimpleString(store.type_of(&key).unwrap_or("none").to_string())
}

/// `MSET key value [key value ...]` — set many keys at once. Always `+OK`.
///
/// All or nothing: every key is checked *before* anything is written, so a bad
/// key means nothing was stored and there is nothing to roll back.
///
/// # Example
///
/// ```text
/// MSET a 1 b 2  ->  +OK
/// MSET a 1 b    ->  -ERR wrong number of arguments for 'mset' command
/// ```
fn cmd_mset(args: Vec<Vec<u8>>, store: &mut Db) -> RespTypes {
    if args.len() % 2 != 0 {
        return wrong_arity("mset");
    }

    let mut pairs = Vec::with_capacity(args.len() / 2);
    let mut iter = args.into_iter();

    while let Some(key) = iter.next() {
        let value = iter.next().unwrap();
        match key_of(&key) {
            Ok(key) => pairs.push((key, value)),
            Err(reply) => return reply,
        }
    }
    for (key, value) in pairs {
        store.set(key, RedisValue::String(value));
    }

    RespTypes::SimpleString("OK".to_string())
}

/// `MGET key [key ...]` — get many keys at once.
///
/// Always one element per key, in the order asked. A missing key gets a nil in
/// its slot, never a skipped slot — otherwise the positions shift and the
/// client can't tell which key gave what. Never fails on a bad key: it just
/// can't exist, so that is a nil too.
///
/// # Example
///
/// ```text
/// MGET a nope b  ->  *3\r\n$1\r\n1\r\n$-1\r\n$1\r\n2\r\n
///
/// which redis-cli prints as:
///     1) "1"
///     2) (nil)
///     3) "2"
/// ```
fn cmd_mget(args: &[Vec<u8>], store: &Db) -> RespTypes {
    if args.is_empty() {
        return wrong_arity("mget");
    }
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        let reply = match key_of(arg) {
            Ok(key) => match store.get(&key) {
                Some(RedisValue::String(bytes)) => BulkString(Some(bytes.clone())),
                None => BulkString(None),
            },
            Err(_) => BulkString(None),
        };
        out.push(reply);
    }

    RespTypes::Array(Some(out))
}

/// Turns argument bytes into a key, or gives back the error frame to reply with.
///
/// Values can be any bytes (a JPEG, say), but keys must be valid UTF-8 because
/// `Db` stores them as `String`. This is the one place that is checked.
///
/// # Example
///
/// ```text
/// key_of(b"foo")   -> Ok("foo")
/// key_of(b"\xff")  -> Err(-ERR key must be valid UTF-8)
/// ```
fn key_of(bytes: &[u8]) -> Result<String, RespTypes> {
    String::from_utf8(bytes.to_vec()).map_err(|_| error("ERR key must be valid UTF-8"))
}

/// The "wrong number of arguments" error, worded exactly as Redis words it.
///
/// Clients match on this text, so it lives in one place.
///
/// # Example
///
/// ```text
/// wrong_arity("get")  ->  -ERR wrong number of arguments for 'get' command
/// ```
fn wrong_arity(command: &str) -> RespTypes {
    error(&format!(
        "ERR wrong number of arguments for '{command}' command"
    ))
}

/// Wraps a message as a RESP error frame.
///
/// # Example
///
/// ```text
/// error("ERR nope")  ->  -ERR nope\r\n
/// ```
fn error(message: &str) -> RespTypes {
    RespTypes::Error(message.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/command_tests.rs"]
mod tests;
