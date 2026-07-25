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
