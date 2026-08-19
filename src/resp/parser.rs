/// A single decoded RESP2 value — the *result* of parsing, not the raw bytes.
///
/// Raw bytes go into `parse()`; a `RespTypes` comes out. Nothing here holds
/// unparsed text.
///
/// Only `BulkString` and `Array` are `Option`, because those are the only two
/// types RESP gives a null form: `$-1\r\n` (key doesn't exist) and `*-1\r\n`.
#[derive(Debug, PartialEq)]
pub enum RespTypes {
    SimpleString(String),
    Error(String),
    Integer(i64),
    /// `Vec<u8>`, not `String`: Redis values are binary-safe.
    BulkString(Option<Vec<u8>>),
    /// Recursive — an array holds other frames, including more arrays.
    Array(Option<Vec<RespTypes>>),
}

/// Why the parser could not produce a `RespTypes`.
#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// Not really an error: the rest of the message hasn't arrived yet.
    /// Read more bytes and try again.
    Incomplete,
    /// The client sent something that isn't valid RESP. Drop the connection.
    Protocol(String),
}

/// Reads one line, up to the next `\r\n`.
///
/// `b"+OK\r\n"` returns `b"+OK"` and leaves `pos` at 5.
/// The marker byte is kept. On an error `pos` does not move.
fn read_line<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8], ParseError> {
    let mut i = *pos;

    while i < buf.len() && buf[i] != b'\r' {
        i += 1;
    }

    // Either no '\r' at all, or it's the last byte and '\n' hasn't arrived.
    if i + 1 >= buf.len() {
        return Err(ParseError::Incomplete);
    }

    // In RESP a '\r' is only ever legal as part of '\r\n'.
    if buf[i + 1] != b'\n' {
        return Err(ParseError::Protocol("expected \\n after \\r".into()));
    }

    let line = &buf[*pos..i];
    *pos = i + 2;
    Ok(line)
}

/// Reads one *command* from a client.
///
/// This is what the server calls. `parse` is the general RESP value reader and
/// is used for replies too
///
/// The two formats split here, at the top level, and nowhere else:
/// a leading `*` is a RESP array, anything else is an inline command.
pub fn parse_command(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    if *pos >= buf.len() {
        return Err(ParseError::Incomplete);
    }
    match buf[*pos] {
        b'*' => parse(buf, pos),
        _ => parse_inline(buf, pos),
    }
}

/// Reads one RESP value starting at `pos` and moves `pos` past it.
///
/// The first byte says which type it is. We only look at it here,
/// we do not skip it. Each helper removes it later.
///
/// Call this again with the same `pos` to read the next value.
pub fn parse(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    // Not `buf.is_empty()`: with back-to-back frames the buffer is full but
    // the cursor can already sit at the end.
    if *pos >= buf.len() {
        return Err(ParseError::Incomplete);
    }
    match buf[*pos] {
        b'+' => parse_simple_string(buf, pos),
        b'-' => parse_error(buf, pos),
        b':' => parse_integer(buf, pos),
        b'$' => parse_bulk_string(buf, pos),
        b'*' => parse_array(buf, pos),
        _ => Err(ParseError::Protocol("unknown RESP type".into())),
    }
}

/// Reads one line and throws away the marker byte.
///
/// `b"+OK\r\n"` gives `"OK"`.
///
/// Shared by the three types that are just one line of text.
/// The line always has a marker because `parse` checked for one first,
/// so `line[1..]` cannot panic.
fn parse_line_string(buf: &[u8], pos: &mut usize) -> Result<String, ParseError> {
    let line = read_line(buf, pos)?;
    let s = std::str::from_utf8(&line[1..])
        .map_err(|_| ParseError::Protocol("invalid UTF-8".into()))?;
    Ok(s.to_string())
}

/// `b"+OK\r\n"` -> `SimpleString("OK")`
///
/// Used for short replies like `+OK` and `+PONG`.
fn parse_simple_string(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    Ok(RespTypes::SimpleString(parse_line_string(buf, pos)?))
}

/// `b"-ERR unknown command\r\n"` -> `Error("ERR unknown command")`
///
/// This is an error the server sends to a client, not a parse failure.
/// Reading it worked fine, so it comes back as `Ok`.
fn parse_error(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    Ok(RespTypes::Error(parse_line_string(buf, pos)?))
}

/// `b":42\r\n"` -> `Integer(42)`. Negatives work too: `b":-1\r\n"` -> `Integer(-1)`.
///
/// The number arrives as text, so it has to go through `.parse()`.
/// Something like `:abc` is bad input, so that is a `Protocol` error.
fn parse_integer(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    let result = parse_line_string(buf, pos)?;
    let n: i64 = result
        .parse()
        .map_err(|_| ParseError::Protocol("invalid number parsing error".into()))?;

    Ok(RespTypes::Integer(n))
}

/// `b"$5\r\nhello\r\n"` -> `BulkString(Some(b"hello"))`, `b"$-1\r\n"` -> `BulkString(None)`
///
/// Two lines: a length header, then that many raw bytes.
/// The payload is taken by length, never by scanning for `\r\n`, because the
/// bytes are binary and may contain a `\r\n` of their own.
fn parse_bulk_string(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    // Where we started, so an incomplete frame can be retried from scratch.
    let frame_start = *pos;

    let line = read_line(buf, pos)?;

    // The null bulk string: what GET returns for a key that doesn't exist.
    if &line[1..] == b"-1" {
        return Ok(RespTypes::BulkString(None));
    }

    let length: usize = std::str::from_utf8(&line[1..])
        .map_err(|_| ParseError::Protocol("invalid UTF-8".into()))?
        .parse()
        .map_err(|_| ParseError::Protocol("invalid bulk length".into()))?;

    let start = *pos;
    //Huge length must not wrap the addition around.
    let end = start
        .checked_add(length)
        .ok_or_else(|| ParseError::Protocol("bulk length too large".into()))?;

    // Payload plus its trailing \r\n
    if end + 2 > buf.len() {
        *pos = frame_start;
        return Err(ParseError::Incomplete);
    }

    // Valid only if both bytes are right, so it's bad if *either* is wrong.
    if buf[end] != b'\r' || buf[end + 1] != b'\n' {
        return Err(ParseError::Protocol(
            "bulk string not terminated by CRLF".into(),
        ));
    }

    *pos = end + 2;
    Ok(RespTypes::BulkString(Some(buf[start..end].to_vec())))
}

/// `b"*2\r\n:1\r\n:2\r\n"` -> `Array(Some([Integer(1), Integer(2)]))`,
/// `b"*-1\r\n"` -> `Array(None)`
///
/// A count, then that many whole frames. Reading an element is just `parse`
/// again, which is why nested arrays need no extra code.
///
/// An array is all-or-nothing: if any element is short, the whole frame is
/// `Incomplete` and `pos` goes back to where it started.
fn parse_array(buff: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    let frame_start = *pos;
    let line = read_line(buff, pos)?;

    if &line[1..] == b"-1" {
        return Ok(RespTypes::Array(None));
    }

    let count: usize = std::str::from_utf8(&line[1..])
        .map_err(|_| ParseError::Protocol("invalid UTF-8".into()))?
        .parse()
        .map_err(|_| ParseError::Protocol("invalid array length".into()))?;

    // Every element needs at least 3 bytes on the wire (`+\r\n`), so a count
    // bigger than the bytes left cannot be satisfied. Checking first stops
    if count > buff.len() - *pos {
        *pos = frame_start;
        return Err(ParseError::Incomplete);
    }

    let mut replies: Vec<RespTypes> = Vec::with_capacity(count);

    for _ in 0..count {
        match parse(buff, pos) {
            Ok(frame) => replies.push(frame),
            Err(error) => {
                // If error occurs mid way due to anything we place the position
                // at the start and act like nothing happened
                *pos = frame_start;
                return Err(error);
            }
        }
    }

    Ok(RespTypes::Array(Some(replies)))
}

/// `b"SET foo bar\r\n"` -> `Array([Bulk("SET"), Bulk("foo"), Bulk("bar")])`
///
/// The other format a command can arrive in. Real clients send RESP arrays, but
/// Redis also accepts a bare line of words, so we can type commands into `nc`
/// or `telnet` without hand-encoding anything.
///
/// The frame that comes out is the same shape a RESP array would produce, so
/// `dispatch` never has to know which format the client used.
fn parse_inline(buff: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    let mut command: Vec<RespTypes> = Vec::new();
    let line = read_line(buff, pos)?;
    for word in line.split(|byte| *byte == b' ') {
        // `"SET  foo"` splits into three pieces with an empty one in the
        // middle. Runs of spaces are not an error, they are just nothing.
        if word.is_empty() {
            continue;
        }
        // The same `Vec<u8>` a bulk string would hold. Inline values are still
        // raw bytes; they just cannot contain a space or a newline.
        command.push(RespTypes::BulkString(Some(word.to_vec())));
    }

    if command.is_empty() {
        // A blank line: the client pressed enter. Not a command, not an error.
        // It is already consumed, so read whatever comes after it.
        return parse_command(buff, pos);
    }

    Ok(RespTypes::Array(Some(command)))
}

#[cfg(test)]
#[path = "../../tests/unit/parser_tests.rs"]
mod tests;
