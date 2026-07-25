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
