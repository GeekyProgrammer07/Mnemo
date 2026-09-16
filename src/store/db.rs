use std::collections::HashMap;

use crate::store::value::RedisValue;

/// The keyspace: every key and value this server holds.
///
/// Knows nothing about RESP or sockets — just bytes in, bytes out.
/// A `HashMap` because every operation here is "find this exact key", which it
/// does in O(1) average. It has no order at all, so sorted things (ZSets,
/// key ranges) will need a different structure.
///
/// # Example
///
/// ```text
/// let mut db = Db::default();
/// db.set("greeting".into(), RedisValue::String(b"hello".to_vec()));
/// db.get("greeting");    // Some(String(b"hello"))
/// db.exists("greeting"); // true
/// ```
#[derive(Default)]
pub struct Db {
    map: HashMap<String, RedisValue>,
}
// get, set, del, exists, type_of.

impl Db {
    /// Looks up a key. `None` means there is no such key.
    ///
    /// Hands back a reference, not a copy — a value could be megabytes.
    pub fn get(&self, key: &str) -> Option<&RedisValue> {
        self.map.get(key)
    }

    /// Stores a value, replacing anything already under that key.
    ///
    /// Returns the *old* value if there was one. `SET` ignores that, but
    /// `GETSET` and `SET .. NX` will want it later.
    ///
    /// # Example
    ///
    /// ```text
    /// db.set("k".into(), String(b"1"));  // -> None      (nothing was there)
    /// db.set("k".into(), String(b"2"));  // -> Some("1")  (overwrote it)
    /// ```
    pub fn set(&mut self, key: String, value: RedisValue) -> Option<RedisValue> {
        self.map.insert(key, value)
    }

    /// Removes a key and hands back what was stored, so the caller can tell
    /// "deleted one" from "there was nothing to delete". `DEL` counts with this.
    pub fn del(&mut self, key: &str) -> Option<RedisValue> {
        self.map.remove(key)
    }

    /// Is this key present? Cheaper than `get` when the value isn't needed.
    pub fn exists(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// The stored value's type name, or `None` when the key is missing.
    ///
    /// Redis reports a missing key as the word `none`, but that is the
    /// command's wording to choose, not the store's.
    ///
    /// # Example
    ///
    /// ```text
    /// db.type_of("greeting")  // -> Some("string")
    /// db.type_of("nope")      // -> None    (TYPE turns this into +none)
    /// ```
    pub fn type_of(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|value| value.type_name())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/db_tests.rs"]
mod tests;
