use crate::{
    resp::parser::RespTypes,
    store::{db::Db, value::RedisValue},
};

/// Turns a frame the client sent into the frame we should send back.
///
/// Clients always send an array of bulk strings: element 0 is the command name,
/// the rest are its arguments. Anything else is a client mistake, and a client
/// mistake is a normal `-ERR` reply
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
        other => error(&format!("ERR unknown command '{other}'")),
    }
}

fn cmd_ping(mut args: Vec<Vec<u8>>) -> RespTypes {
    match args.len() {
        0 => RespTypes::SimpleString("PONG".to_string()),
        // `PING hello` replies `hello` instead of `PONG`.
        1 => RespTypes::BulkString(Some(args.remove(0))),
        _ => wrong_arity("ping"),
    }
}

fn cmd_echo(mut args: Vec<Vec<u8>>) -> RespTypes {
    match args.len() {
        1 => RespTypes::BulkString(Some(args.remove(0))),
        _ => wrong_arity("echo"),
    }
}

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

/// Turns the raw bytes of an argument into a key, or gives back the error to
/// reply with.
///
/// Arguments arrive as `Vec<u8>` because a *value* can be any bytes at all —
/// a JPEG, a compressed blob, anything. But `Db` stores keys as `String`, so
/// a key has to be valid UTF-8. This is where that check happens, once, in
/// one place, instead of in every command.
///
/// Real Redis allows binary keys too. Requiring text is a simplification: keys
/// are names in practice, and `String` keeps the store simpler to read.
fn key_of(bytes: &[u8]) -> Result<String, RespTypes> {
    String::from_utf8(bytes.to_vec()).map_err(|_| error("ERR key must be valid UTF-8"))
}

/// [Redis](https://redis.io/docs/latest/develop/reference/modules/#arity-and-type-checkswords) uses this identically for every command,
fn wrong_arity(command: &str) -> RespTypes {
    error(&format!(
        "ERR wrong number of arguments for '{command}' command"
    ))
}

fn error(message: &str) -> RespTypes {
    RespTypes::Error(message.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/command_tests.rs"]
mod tests;
