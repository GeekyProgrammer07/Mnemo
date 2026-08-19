#[derive(Debug, PartialEq)]
pub enum RedisValue {
    // Redis uses its own SDS(Simple Dynamic String) to store strings
    // it has the structure
    // | len | alloc | flags(SDS types) | buff[] (the actual data in utf8 encoding)
    // In rust the vec does the same thing for us
    String(Vec<u8>),
}

impl RedisValue {
    /// The name `TYPE` reports. Lowercase and exact: clients match on these.
    pub fn type_name(&self) -> &str {
        match self {
            RedisValue::String(_) => "string",
        }
    }
}
