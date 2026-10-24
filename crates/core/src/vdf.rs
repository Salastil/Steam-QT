//! Minimal text VDF (KeyValues) writer, plus re-exports of steamroom's parser.
//!
//! Steam's on-disk files (`appmanifest_*.acf`, `libraryfolders.vdf`) are text KeyValues with
//! tab indentation; we write them in the same shape so the official client, Protontricks and
//! other tools can read libraries managed by Steam-QT.

use std::fmt::Write;
pub use steamroom::types::{KeyValue, KvValue};

/// Ordered tree for writing. (steamroom's `KeyValue` uses a `BTreeMap`, which would reorder keys.)
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Value(String),
    Table(Vec<(String, Node)>),
}

impl Node {
    pub fn table() -> Self {
        Node::Table(Vec::new())
    }

    pub fn set(&mut self, key: impl Into<String>, value: Node) -> &mut Self {
        if let Node::Table(entries) = self {
            let key = key.into();
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
        }
        self
    }

    pub fn val(&mut self, key: impl Into<String>, value: impl ToString) -> &mut Self {
        self.set(key, Node::Value(value.to_string()))
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_node(out: &mut String, key: &str, node: &Node, depth: usize) {
    let indent = "\t".repeat(depth);
    match node {
        Node::Value(v) => {
            let _ = writeln!(out, "{indent}\"{}\"\t\t\"{}\"", escape(key), escape(v));
        }
        Node::Table(entries) => {
            let _ = writeln!(out, "{indent}\"{}\"", escape(key));
            let _ = writeln!(out, "{indent}{{");
            for (k, v) in entries {
                write_node(out, k, v, depth + 1);
            }
            let _ = writeln!(out, "{indent}}}");
        }
    }
}

pub fn to_string(root_key: &str, root: &Node) -> String {
    let mut out = String::new();
    write_node(&mut out, root_key, root, 0);
    out
}

pub fn parse(text: &str) -> Option<KeyValue> {
    steamroom::types::key_value::parse_text_kv(text).ok()
}

/// String value of a child, accepting numeric encodings too.
pub fn get_str(kv: &KeyValue, key: &str) -> Option<String> {
    match &kv.get(key)?.value {
        KvValue::String(s) => Some(s.clone()),
        KvValue::Int32(v) => Some(v.to_string()),
        KvValue::UInt64(v) => Some(v.to_string()),
        KvValue::Int64(v) => Some(v.to_string()),
        _ => None,
    }
}

pub fn get_u64(kv: &KeyValue, key: &str) -> Option<u64> {
    get_str(kv, key)?.trim().parse().ok()
}

pub fn children(kv: Option<&KeyValue>) -> Vec<(&String, &KeyValue)> {
    match kv.map(|k| &k.value) {
        Some(KvValue::Children(map)) => map.iter().collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut root = Node::table();
        root.val("appid", 620).val("name", "Portal \"2\"");
        let mut depots = Node::table();
        let mut d = Node::table();
        d.val("manifest", "123").val("size", 456);
        depots.set("621", d);
        root.set("InstalledDepots", depots);
        let text = to_string("AppState", &root);
        assert!(text.starts_with("\"AppState\"\n{\n\t\"appid\"\t\t\"620\"\n"));
        let kv = parse(&text).expect("parse");
        assert_eq!(get_str(&kv, "name").as_deref(), Some("Portal \"2\""));
        let depot = kv
            .get("InstalledDepots")
            .and_then(|d| d.get("621"))
            .unwrap();
        assert_eq!(get_u64(depot, "size"), Some(456));
    }
}
