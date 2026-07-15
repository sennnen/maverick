//! The per-device key-value store handle a codec sees. It is scoped to one physical device by the
//! caller, so one strap's learned values (the gen4 skin-temp anchor is the standing example) can
//! never leak into another's. The durable implementation lives in mav-store; codecs only ever see
//! this trait.

use serde_json::Value;

pub trait DeviceKv {
    fn get(&self, key: &str) -> Option<Value>;
    fn put(&mut self, key: &str, value: Value);
}

/// In-memory implementation for tests and replay.
#[derive(Default, Debug, Clone)]
pub struct MemoryKv {
    entries: std::collections::BTreeMap<String, Value>,
}

impl MemoryKv {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DeviceKv for MemoryKv {
    fn get(&self, key: &str) -> Option<Value> {
        self.entries.get(key).cloned()
    }

    fn put(&mut self, key: &str, value: Value) {
        self.entries.insert(key.to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_then_get_roundtrips() {
        let mut kv = MemoryKv::new();
        assert_eq!(kv.get("anchor_raw"), None);
        kv.put("anchor_raw", serde_json::json!(826));
        assert_eq!(kv.get("anchor_raw"), Some(serde_json::json!(826)));
        kv.put("anchor_raw", serde_json::json!(830));
        assert_eq!(kv.get("anchor_raw"), Some(serde_json::json!(830)));
    }
}
