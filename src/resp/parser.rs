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

#[cfg(test)]
mod tests {
    use super::*;

    // --- read_line ---------------------------------------------------------

    #[test]
    fn read_line_returns_the_line_and_moves_past_crlf() {
        let mut pos = 0;
        assert_eq!(read_line(b"+OK\r\n", &mut pos), Ok(&b"+OK"[..]));
        assert_eq!(pos, 5);
    }

    #[test]
    fn read_line_handles_an_empty_line() {
        let mut pos = 0;
        assert_eq!(read_line(b"\r\n", &mut pos), Ok(&b""[..]));
        assert_eq!(pos, 2);
    }

    #[test]
    fn read_line_on_empty_buffer_does_not_panic() {
        // A client can connect and send nothing. Indexing blindly would crash
        // the whole server here.
        let mut pos = 0;
        assert_eq!(read_line(b"", &mut pos), Err(ParseError::Incomplete));
    }

    #[test]
    fn read_line_without_crlf_is_incomplete() {
        let mut pos = 0;
        assert_eq!(read_line(b"+OK", &mut pos), Err(ParseError::Incomplete));
        // Nothing was consumed, so we can retry after more bytes arrive.
        assert_eq!(pos, 0);
    }

    #[test]
    fn read_line_with_cr_but_no_lf_yet_is_incomplete() {
        // TCP split the message right between \r and \n.
        let mut pos = 0;
        assert_eq!(read_line(b"+OK\r", &mut pos), Err(ParseError::Incomplete));
    }

    #[test]
    fn read_line_rejects_a_lone_cr() {
        // A \r not followed by \n is never valid RESP.
        let mut pos = 0;
        assert!(matches!(
            read_line(b"+O\rK\r\n", &mut pos),
            Err(ParseError::Protocol(_))
        ));
    }

    // --- the three one-line types -----------------------------------------

    #[test]
    fn parses_a_simple_string() {
        let mut pos = 0;
        assert_eq!(
            parse(b"+OK\r\n", &mut pos),
            Ok(RespTypes::SimpleString("OK".to_string()))
        );
        assert_eq!(pos, 5);
    }

    #[test]
    fn parses_an_empty_simple_string() {
        let mut pos = 0;
        assert_eq!(
            parse(b"+\r\n", &mut pos),
            Ok(RespTypes::SimpleString(String::new()))
        );
    }

    #[test]
    fn parses_an_error() {
        let mut pos = 0;
        assert_eq!(
            parse(b"-ERR unknown command\r\n", &mut pos),
            Ok(RespTypes::Error("ERR unknown command".to_string()))
        );
    }

    #[test]
    fn parses_an_integer() {
        let mut pos = 0;
        assert_eq!(parse(b":42\r\n", &mut pos), Ok(RespTypes::Integer(42)));
        assert_eq!(pos, 5);
    }

    #[test]
    fn parses_a_negative_integer() {
        let mut pos = 0;
        assert_eq!(parse(b":-1\r\n", &mut pos), Ok(RespTypes::Integer(-1)));
    }

    #[test]
    fn integer_that_is_not_a_number_is_a_protocol_error() {
        let mut pos = 0;
        assert!(matches!(
            parse(b":abc\r\n", &mut pos),
            Err(ParseError::Protocol(_))
        ));
    }

    // --- bulk strings ------------------------------------------------------

    #[test]
    fn parses_a_bulk_string() {
        let mut pos = 0;
        assert_eq!(
            parse(b"$5\r\nhello\r\n", &mut pos),
            Ok(RespTypes::BulkString(Some(b"hello".to_vec())))
        );
        assert_eq!(pos, 11);
    }

    #[test]
    fn parses_an_empty_bulk_string() {
        let mut pos = 0;
        assert_eq!(
            parse(b"$0\r\n\r\n", &mut pos),
            Ok(RespTypes::BulkString(Some(vec![])))
        );
        assert_eq!(pos, 6);
    }

    #[test]
    fn parses_a_null_bulk_string() {
        // What GET returns when the key is missing.
        let mut pos = 0;
        assert_eq!(parse(b"$-1\r\n", &mut pos), Ok(RespTypes::BulkString(None)));
        assert_eq!(pos, 5);
    }

    #[test]
    fn bulk_string_payload_may_contain_crlf() {
        // The reason the payload is taken by length instead of by scanning:
        // these 5 bytes include a \r\n that is data, not a terminator.
        let mut pos = 0;
        assert_eq!(
            parse(b"$5\r\na\r\nbc\r\n", &mut pos),
            Ok(RespTypes::BulkString(Some(b"a\r\nbc".to_vec())))
        );
    }

    #[test]
    fn bulk_string_payload_may_be_binary() {
        let mut pos = 0;
        assert_eq!(
            parse(b"$3\r\n\xff\x00\xfe\r\n", &mut pos),
            Ok(RespTypes::BulkString(Some(vec![0xff, 0x00, 0xfe])))
        );
    }

    #[test]
    fn half_delivered_bulk_string_is_incomplete_not_a_panic() {
        // Slicing without a bounds check here would crash the whole server.
        let mut pos = 0;
        assert_eq!(parse(b"$5\r\nhel", &mut pos), Err(ParseError::Incomplete));
        // Cursor is back at the start so the frame can be retried.
        assert_eq!(pos, 0);
    }

    #[test]
    fn bulk_string_missing_its_trailing_crlf_is_a_protocol_error() {
        let mut pos = 0;
        assert!(matches!(
            parse(b"$5\r\nhelloXX", &mut pos),
            Err(ParseError::Protocol(_))
        ));
    }

    #[test]
    fn bulk_string_with_only_one_bad_terminator_byte_is_rejected() {
        // Catches `&&` where `||` is meant: here only the first byte is wrong,
        // so an `&&` check would wave this through.
        let mut pos = 0;
        assert!(matches!(
            parse(b"$5\r\nhelloX\n", &mut pos),
            Err(ParseError::Protocol(_))
        ));

        // And the mirror case: only the second byte is wrong.
        let mut pos = 0;
        assert!(matches!(
            parse(b"$5\r\nhello\rX", &mut pos),
            Err(ParseError::Protocol(_))
        ));
    }

    #[test]
    fn negative_bulk_length_other_than_minus_one_is_a_protocol_error() {
        let mut pos = 0;
        assert!(matches!(
            parse(b"$-2\r\nab\r\n", &mut pos),
            Err(ParseError::Protocol(_))
        ));
    }

    #[test]
    fn reads_two_bulk_strings_from_one_buffer() {
        let buf = b"$4\r\nECHO\r\n$5\r\nhello\r\n";
        let mut pos = 0;

        assert_eq!(
            parse(buf, &mut pos),
            Ok(RespTypes::BulkString(Some(b"ECHO".to_vec())))
        );
        assert_eq!(
            parse(buf, &mut pos),
            Ok(RespTypes::BulkString(Some(b"hello".to_vec())))
        );
        assert_eq!(pos, buf.len());
    }

    // --- arrays ------------------------------------------------------------

    #[test]
    fn parses_a_command_array() {
        // What a real client actually sends for: ECHO hello
        let buf = b"*2\r\n$4\r\nECHO\r\n$5\r\nhello\r\n";
        let mut pos = 0;
        assert_eq!(
            parse(buf, &mut pos),
            Ok(RespTypes::Array(Some(vec![
                RespTypes::BulkString(Some(b"ECHO".to_vec())),
                RespTypes::BulkString(Some(b"hello".to_vec())),
            ])))
        );
        assert_eq!(pos, buf.len());
    }

    #[test]
    fn parses_an_empty_array() {
        let mut pos = 0;
        assert_eq!(
            parse(b"*0\r\n", &mut pos),
            Ok(RespTypes::Array(Some(vec![])))
        );
        assert_eq!(pos, 4);
    }

    #[test]
    fn parses_a_null_array() {
        let mut pos = 0;
        assert_eq!(parse(b"*-1\r\n", &mut pos), Ok(RespTypes::Array(None)));
        assert_eq!(pos, 5);
    }

    #[test]
    fn parses_a_nested_array() {
        // Nesting needs no extra code: reading an element is just `parse`.
        let mut pos = 0;
        assert_eq!(
            parse(b"*1\r\n*2\r\n:5\r\n+OK\r\n", &mut pos),
            Ok(RespTypes::Array(Some(vec![RespTypes::Array(Some(vec![
                RespTypes::Integer(5),
                RespTypes::SimpleString("OK".to_string()),
            ]))])))
        );
    }

    #[test]
    fn parses_an_array_of_mixed_types() {
        let mut pos = 0;
        assert_eq!(
            parse(b"*3\r\n:1\r\n$2\r\nhi\r\n-ERR x\r\n", &mut pos),
            Ok(RespTypes::Array(Some(vec![
                RespTypes::Integer(1),
                RespTypes::BulkString(Some(b"hi".to_vec())),
                RespTypes::Error("ERR x".to_string()),
            ])))
        );
    }

    #[test]
    fn half_delivered_array_rewinds_the_cursor() {
        // The count says 2 but only one element arrived. Without the rewind,
        // pos would be left after `:1\r\n` and the retry would resume
        // mid-array, misreading everything from there on.
        let mut pos = 0;
        assert_eq!(
            parse(b"*2\r\n:1\r\n", &mut pos),
            Err(ParseError::Incomplete)
        );
        assert_eq!(pos, 0);
    }

    #[test]
    fn array_element_that_is_garbage_is_a_protocol_error() {
        let mut pos = 0;
        assert!(matches!(
            parse(b"*1\r\n?bad\r\n", &mut pos),
            Err(ParseError::Protocol(_))
        ));
    }

    #[test]
    fn absurd_array_count_does_not_allocate() {
        // 12 bytes claiming a billion elements. Reserving for that count up
        // front would try for gigabytes of memory.
        let mut pos = 0;
        assert_eq!(
            parse(b"*999999999\r\n", &mut pos),
            Err(ParseError::Incomplete)
        );
        assert_eq!(pos, 0);
    }

    // --- parse dispatch ----------------------------------------------------

    #[test]
    fn unknown_marker_is_a_protocol_error() {
        let mut pos = 0;
        assert!(matches!(
            parse(b"?what\r\n", &mut pos),
            Err(ParseError::Protocol(_))
        ));
    }

    #[test]
    fn empty_buffer_is_incomplete() {
        let mut pos = 0;
        assert_eq!(parse(b"", &mut pos), Err(ParseError::Incomplete));
    }

    #[test]
    fn cursor_at_end_of_a_full_buffer_is_incomplete() {
        // The bug `buf.is_empty()` would have missed: buffer is not empty,
        // but there is nothing left to read, so `buf[*pos]` would panic.
        let mut pos = 5;
        assert_eq!(parse(b"+OK\r\n", &mut pos), Err(ParseError::Incomplete));
    }

    #[test]
    fn reads_two_frames_from_one_buffer() {
        // This is what proves `pos` is being carried correctly. If parse left
        // the cursor on the \r, the second call would loop forever.
        let buf = b"+OK\r\n:42\r\n";
        let mut pos = 0;

        assert_eq!(
            parse(buf, &mut pos),
            Ok(RespTypes::SimpleString("OK".to_string()))
        );
        assert_eq!(parse(buf, &mut pos), Ok(RespTypes::Integer(42)));
        assert_eq!(pos, buf.len());
    }
}
