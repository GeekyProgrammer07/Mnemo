//! Unit tests for `src/store/db.rs`.
//!
//! Pulled into the crate by the `#[path]` stub at the bottom of that file, so
//! these are still part of the `store::db` module: they run with `cargo test`,
//! they see `use super::*`, and they can reach private items. Only the file they
//! live in changed.

use super::*;

fn string(s: &str) -> RedisValue {
    RedisValue::String(s.as_bytes().to_vec())
}

#[test]
fn get_returns_what_set_stored() {
    let mut db = Db::default();
    db.set("foo".to_string(), string("bar"));
    assert_eq!(db.get("foo"), Some(&string("bar")));
}

#[test]
fn get_on_a_missing_key_is_none() {
    let db = Db::default();
    assert_eq!(db.get("nope"), None);
}

#[test]
fn set_overwrites_and_hands_back_the_old_value() {
    let mut db = Db::default();
    assert_eq!(db.set("k".to_string(), string("one")), None);
    assert_eq!(db.set("k".to_string(), string("two")), Some(string("one")));
    assert_eq!(db.get("k"), Some(&string("two")));
}

#[test]
fn del_removes_the_key_and_returns_what_was_there() {
    let mut db = Db::default();
    db.set("k".to_string(), string("v"));
    assert_eq!(db.del("k"), Some(string("v")));
    assert_eq!(db.del("k"), None);
    assert!(!db.exists("k"));
}

#[test]
fn exists_tracks_set_and_del() {
    let mut db = Db::default();
    assert!(!db.exists("k"));
    db.set("k".to_string(), string("v"));
    assert!(db.exists("k"));
}

#[test]
fn type_of_names_the_kind_of_value() {
    let mut db = Db::default();
    db.set("k".to_string(), string("v"));
    assert_eq!(db.type_of("k"), Some("string"));
    assert_eq!(db.type_of("missing"), None);
}

#[test]
fn values_do_not_have_to_be_valid_utf8() {
    // A key can hold a JPEG. This is why the value is Vec<u8>, not String.
    let mut db = Db::default();
    let bytes = RedisValue::String(vec![0xff, 0x00, 0xfe]);
    db.set(
        "blob".to_string(),
        RedisValue::String(vec![0xff, 0x00, 0xfe]),
    );
    assert_eq!(db.get("blob"), Some(&bytes));
}
