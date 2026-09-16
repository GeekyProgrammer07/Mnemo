/// One value stored under a key.
///
/// Redis is a *typed* store: a value is a string, list, hash, set or sorted
/// set, and commands check the type before running. Each new data type adds a
/// variant here. Only `String` exists so far.
///
/// # Example
///
/// ```text
/// SET greeting hello  ->  RedisValue::String(b"hello".to_vec())
/// TYPE greeting       ->  "string"
/// ```
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
    ///
    /// Redis's full set is `string`, `list`, `set`, `zset`, `hash`, `stream`.
    /// There is no `"none"` here — a missing key is not a value, so that word
    /// is `cmd_type`'s job.
    ///
    /// # Example
    ///
    /// ```text
    /// RedisValue::String(b"hi".to_vec()).type_name()  ==  "string"
    /// ```
    pub fn type_name(&self) -> &str {
        match self {
            RedisValue::String(_) => "string",
        }
    }
}
