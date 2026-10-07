//! Format adapters produce one strict resource tree for the importer.
use anyhow::{Context, Result, bail, ensure};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor},
};
use std::{collections::BTreeMap, fmt, path::Path};

pub enum Node {
    String(String),
    Table(BTreeMap<String, Node>),
}

impl Node {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }
}

// Deserialize directly rather than through generic values, which can silently
// discard duplicate keys. All formats enforce the same string/object schema.
impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ResourceVisitor;
        impl<'de> Visitor<'de> for ResourceVisitor {
            type Value = Node;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a resource string or object")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Node, E> {
                Ok(Node::String(value.into()))
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Node, M::Error> {
                let mut result = BTreeMap::new();
                while let Some(key) = map.next_key::<Node>()? {
                    let Node::String(key) = key else {
                        return Err(de::Error::custom("Resource keys must be strings"));
                    };
                    if result.contains_key(&key) {
                        return Err(de::Error::custom(format!("Duplicate key: {key}")));
                    }
                    result.insert(key, map.next_value()?);
                }
                Ok(Node::Table(result))
            }
        }
        deserializer.deserialize_any(ResourceVisitor)
    }
}

trait ImportAdapter {
    fn parse(&self, text: &str) -> Result<Node>;
}
struct TomlAdapter;
struct YamlAdapter;
struct JsonAdapter;
impl ImportAdapter for TomlAdapter {
    fn parse(&self, text: &str) -> Result<Node> {
        Ok(toml::from_str(text)?)
    }
}
impl ImportAdapter for YamlAdapter {
    fn parse(&self, text: &str) -> Result<Node> {
        Ok(serde_yaml_ng::from_str(text)?)
    }
}
impl ImportAdapter for JsonAdapter {
    fn parse(&self, text: &str) -> Result<Node> {
        Ok(serde_json::from_str(text)?)
    }
}

pub fn parse(path: &Path, text: &str) -> Result<Node> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let adapter: &dyn ImportAdapter = match extension.as_str() {
        "toml" => &TomlAdapter,
        "yaml" | "yml" => &YamlAdapter,
        "json" => &JsonAdapter,
        _ => bail!(
            "Unsupported config format in {}; use .toml, .yaml, .yml, or .json",
            path.display()
        ),
    };
    let node = adapter
        .parse(text)
        .with_context(|| format!("Invalid {extension} in {}", path.display()))?;
    ensure!(
        matches!(node, Node::Table(_)),
        "Config root must be an object in {}",
        path.display()
    );
    Ok(node)
}

/// Read comment metadata using parser-provided key spans, so text inside
/// multiline strings cannot be mistaken for resource declarations.
pub fn toml_hints(text: &str) -> Result<BTreeMap<Vec<String>, String>> {
    struct Keys(Vec<(toml::Spanned<String>, Keys)>);
    impl<'de> Deserialize<'de> for Keys {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct KeyVisitor;
            impl<'de> Visitor<'de> for KeyVisitor {
                type Value = Keys;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a resource string or table")
                }
                fn visit_str<E: de::Error>(self, _: &str) -> Result<Keys, E> {
                    Ok(Keys(vec![]))
                }
                fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Keys, M::Error> {
                    let mut keys = vec![];
                    while let Some(entry) = map.next_entry()? {
                        keys.push(entry);
                    }
                    Ok(Keys(keys))
                }
            }
            deserializer.deserialize_any(KeyVisitor)
        }
    }
    fn collect(
        tree: Keys,
        text: &str,
        path: &mut Vec<String>,
        hints: &mut BTreeMap<Vec<String>, String>,
    ) {
        for (key, children) in tree.0 {
            let before = &text[..key.span().start];
            let (previous, prefix) = before.rsplit_once('\n').unwrap_or(("", before));
            path.push(key.into_inner());
            // Inline table members share their parent's line, not its hint.
            if !prefix.contains('=')
                && let Some(hint) = previous
                    .rsplit('\n')
                    .next()
                    .and_then(|line| line.trim().strip_prefix("# hint:"))
                    .map(str::trim)
                    .filter(|hint| !hint.is_empty())
            {
                hints.insert(path.clone(), hint.to_owned());
            }
            collect(children, text, path, hints);
            path.pop();
        }
    }
    let tree = toml::from_str(text)?;
    let mut hints = BTreeMap::new();
    collect(tree, text, &mut vec![], &mut hints);
    Ok(hints)
}

/// YAML key events carry line positions and exclude comments inside scalar bodies.
pub fn yaml_hints(text: &str) -> Result<BTreeMap<Vec<String>, String>> {
    use yaml_rust2::parser::{Event, Parser};
    let lines: Vec<_> = text.lines().collect();
    let mut parser = Parser::new_from_str(text);
    let mut keys: Vec<Option<String>> = vec![];
    let mut hints = BTreeMap::new();
    loop {
        let (event, marker) = parser.next_token()?;
        match event {
            Event::MappingStart(..) => keys.push(None),
            Event::MappingEnd => {
                keys.pop();
                if let Some(key) = keys.last_mut() {
                    *key = None;
                }
            }
            Event::Scalar(value, ..) => {
                if let Some(key) = keys.last_mut() {
                    if key.is_some() {
                        *key = None;
                    } else {
                        *key = Some(value);
                        // Only a key beginning a line can follow a hint comment.
                        let own_line = lines.get(marker.line() - 1).is_some_and(|line| {
                            line.chars().take(marker.col()).all(char::is_whitespace)
                        });
                        if own_line
                            && let Some(hint) = marker
                                .line()
                                .checked_sub(2)
                                .and_then(|line| lines.get(line))
                                .and_then(|line| line.trim().strip_prefix("# hint:"))
                                .map(str::trim)
                                .filter(|hint| !hint.is_empty())
                        {
                            hints.insert(
                                keys.iter().filter_map(Clone::clone).collect(),
                                hint.to_owned(),
                            );
                        }
                    }
                }
            }
            Event::Alias(_) => {
                if let Some(key) = keys.last_mut() {
                    *key = None;
                }
            }
            Event::StreamEnd => break,
            _ => {}
        }
    }
    Ok(hints)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_shapes_duplicates_and_trailing_documents() {
        for (ext, text) in [
            ("json", "{\"x\":true}"),
            ("json", "{\"x\":null}"),
            ("json", "{\"x\":[]}"),
            ("json", "[]"),
            ("json", "\"https://example.com\""),
            ("json", "{\"x\":\"a\",\"x\":\"b\"}"),
            ("json", "{} {}"),
            ("json", "{broken"),
            ("yaml", "x: true"),
            ("yaml", "x: 42"),
            ("yaml", "x: null"),
            ("yaml", "x: []"),
            ("yaml", "[]"),
            ("yaml", "42: value"),
            ("yaml", "x: a\nx: b"),
            ("yaml", "x: a\n---\nx: b"),
            ("yaml", "x: !custom hello"),
            ("txt", "x = 'a'"),
        ] {
            assert!(
                parse(Path::new(&format!("input.{ext}")), text).is_err(),
                "{ext}: {text}"
            );
        }
        assert!(parse(Path::new("input.YML"), "x: '42'").is_ok());
    }
}
