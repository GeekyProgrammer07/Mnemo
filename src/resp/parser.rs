#[derive(Debug, PartialEq)]
pub enum RespTypes {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<Vec<u8>>),
    Array(Option<Vec<RespTypes>>),
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Incomplete,
    Protocol(String),
}

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
        _ => Err(ParseError::Protocol("unknown RESP type".into())),
    }
}

fn parse_line_string(buf: &[u8], pos: &mut usize) -> Result<String, ParseError> {
    let line = read_line(buf, pos)?;
    let s = std::str::from_utf8(&line[1..])
        .map_err(|_| ParseError::Protocol("invalid UTF-8".into()))?;
    Ok(s.to_string())
}

fn parse_simple_string(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    Ok(RespTypes::SimpleString(parse_line_string(buf, pos)?))
}

fn parse_error(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    Ok(RespTypes::Error(parse_line_string(buf, pos)?))
}

fn parse_integer(buf: &[u8], pos: &mut usize) -> Result<RespTypes, ParseError> {
    let result = parse_line_string(buf, pos)?;
    let n: i64 = result
        .parse()
        .map_err(|_| ParseError::Protocol("invalid number parsing error".into()))?;

    Ok(RespTypes::Integer(n))
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
