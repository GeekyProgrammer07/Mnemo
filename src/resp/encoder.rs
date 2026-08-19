use crate::resp::parser::RespTypes;

/// Turns a `RespTypes` back into the bytes that go out on the socket.
///
/// The opposite of `parse`. There is no `Result` because there is nothing to
/// reject: the value is already valid, so encoding cannot fail.
pub fn encode(value: &RespTypes) -> Vec<u8> {
    match value {
        RespTypes::SimpleString(s) => format!("+{}\r\n", s).into_bytes(),

        RespTypes::Error(s) => format!("-{}\r\n", s).into_bytes(),

        RespTypes::Integer(n) => format!(":{}\r\n", n).into_bytes(),

        RespTypes::BulkString(Some(bytes)) => {
            let mut out = Vec::new();
            out.extend_from_slice(format!("${}\r\n", bytes.len()).as_bytes());
            out.extend_from_slice(bytes);
            out.extend_from_slice(b"\r\n");
            out
        }

        RespTypes::BulkString(None) => b"$-1\r\n".to_vec(),

        RespTypes::Array(Some(items)) => {
            let mut out = Vec::new();
            out.extend_from_slice(format!("*{}\r\n", items.len()).as_bytes());
            for item in items {
                out.extend_from_slice(&encode(item));
            }
            out
        }

        RespTypes::Array(None) => b"*-1\r\n".to_vec(),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/encoder_tests.rs"]
mod tests;
