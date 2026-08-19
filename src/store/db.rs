use std::collections::HashMap;

use crate::store::value::RedisValue;

#[derive(Default)]
pub struct Db {
    map: HashMap<String, RedisValue>,
}
// get, set, del, exists, type_of.

impl Db {
    pub fn get(&self, key: &str) -> Option<&RedisValue> {
        self.map.get(key)
    }

    pub fn set(&mut self, key: String, value: RedisValue) -> Option<RedisValue> {
        self.map.insert(key, value)
    }

    pub fn del(&mut self, key: &str) -> Option<RedisValue> {
        self.map.remove(key)
    }

    pub fn exists(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// `None` when the key is missing — Redis reports that as `none`, but that
    /// is the command's wording to choose, not the store's.
    pub fn type_of(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|value| value.type_name())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/db_tests.rs"]
mod tests;
