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

