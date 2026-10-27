//! Format adapters produce one strict resource tree for the importer.
use anyhow::{Context, Result, bail, ensure};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use std::{collections::BTreeMap, fmt, path::Path};

#[derive(Clone, Debug)]
pub enum Node {
    Bool(bool),
    String(String),
    Table(BTreeMap<String, Node>),
    Array(Vec<Node>),
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
// discard duplicate keys. Booleans are retained for metadata; the importer
// enforces where they can appear.
impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ResourceVisitor;
        impl<'de> Visitor<'de> for ResourceVisitor {
            type Value = Node;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string, object, or array")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Node, E> {
                Ok(Node::Bool(value))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Node, E> {
                Ok(Node::String(value.into()))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Node, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(Node::Array(values))
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
    struct Keys(Vec<(toml::Spanned<String>, toml::Spanned<Keys>)>);
    impl<'de> Deserialize<'de> for Keys {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct KeyVisitor;
            impl<'de> Visitor<'de> for KeyVisitor {
                type Value = Keys;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a resource string or table")
                }
                fn visit_bool<E: de::Error>(self, _: bool) -> Result<Keys, E> {
                    Ok(Keys(vec![]))
                }
                fn visit_str<E: de::Error>(self, _: &str) -> Result<Keys, E> {
                    Ok(Keys(vec![]))
                }
                fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Keys, S::Error> {
                    while seq.next_element::<de::IgnoredAny>()?.is_some() {}
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
            // Only assignments have a value after the key. Table spans can
            // encompass their children, whose trailing comments belong to them.
            let inline_hint = text
                .get(key.span().end..children.span().start)
                .filter(|between| between.contains('=') && !between.contains('\n'))
                .and_then(|_| text[children.span().end..].split('\n').next())
                .and_then(|line| line.trim().strip_prefix("# hint:"))
                .map(str::trim)
                .filter(|hint| !hint.is_empty());
            path.push(key.into_inner());
            // Inline table members share their parent's line, not its hint.
            if !prefix.contains('=')
                && let Some(hint) = inline_hint.or_else(|| {
                    previous
                        .rsplit('\n')
                        .next()
                        .and_then(|line| line.trim().strip_prefix("# hint:"))
                        .map(str::trim)
                        .filter(|hint| !hint.is_empty())
                })
            {
                hints.insert(path.clone(), hint.to_owned());
            }
            collect(children.into_inner(), text, path, hints);
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
    use yaml_rust2::scanner::TScalarStyle;

    // Validate the text before a possible comment as a complete scalar. This
    // distinguishes real comments from hashes inside quoted or plain strings.
    fn inline_hint<'a>(line: &'a str, value: &str) -> Option<&'a str> {
        for (index, _) in line.match_indices('#') {
            let prefix = &line[..index];
            if prefix.ends_with([' ', '\t'])
                && serde_yaml_ng::from_str::<String>(prefix).ok().as_deref() == Some(value)
            {
                return line[index..]
                    .strip_prefix("# hint:")
                    .map(str::trim)
                    .filter(|hint| !hint.is_empty());
            }
        }
        None
    }

    let lines: Vec<_> = text.lines().collect();
    let mut parser = Parser::new_from_str(text);
    let mut keys: Vec<Option<String>> = vec![];
    let mut hints = BTreeMap::new();
    let mut sequence_depth = 0;
    loop {
        let (event, marker) = parser.next_token()?;
        if matches!(event, Event::SequenceStart(..)) {
            sequence_depth += 1;
            continue;
        }
        if matches!(event, Event::SequenceEnd) {
            sequence_depth -= 1;
            if sequence_depth == 0
                && let Some(key) = keys.last_mut()
            {
                *key = None;
            }
            continue;
        }
        if sequence_depth > 0 {
            continue;
        }
        match event {
            Event::MappingStart(..) => keys.push(None),
            Event::MappingEnd => {
                keys.pop();
                if let Some(key) = keys.last_mut() {
                    *key = None;
                }
            }
            Event::Scalar(value, style, ..) => {
                let path = keys.iter().filter_map(Clone::clone).collect::<Vec<_>>();
                if let Some(key) = keys.last_mut() {
                    if key.is_some() {
                        if !matches!(style, TScalarStyle::Literal | TScalarStyle::Folded)
                            && let Some(line) = lines.get(marker.line() - 1)
                            && let Some(start) =
                                line.char_indices().nth(marker.col()).map(|(i, _)| i)
                            && let Some(hint) = inline_hint(&line[start..], &value)
                        {
                            hints.insert(path, hint.to_owned());
                        }
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
    fn yaml_inline_hints_respect_scalar_boundaries() {
        let text = r##"web:
  plain: https://example.com # hint: Plain value
  'ø:key': "https://example.com/# hint: inside" # hint: Quoted value
  escaped: "say \"hello\" # hint: inside" # hint: Escaped quote
  single: 'it''s # hint: inside' # hint: Single quote
  # hint: Above
  both: ./file # hint: Inline wins
  # hint: Keep above
  empty: ./file # hint:
  fragment: https://example.com/#hint:fragment
  ordinary: ./file # ordinary comment # hint: Not metadata
  quoted: "./file # hint: Not metadata"
  multiline: "first
    # hint: Not metadata
    last"
  nested:
    page: ./file # hint: Nested page
  flow: {page: ./file} # hint: Not the child hint
# hint: Task hint
task:
  run: | # hint: Not a resource hint
    echo hello # hint: Not metadata
"##;
        let expected = BTreeMap::from([
            (vec!["web".into(), "plain".into()], "Plain value".into()),
            (vec!["web".into(), "ø:key".into()], "Quoted value".into()),
            (vec!["web".into(), "escaped".into()], "Escaped quote".into()),
            (vec!["web".into(), "single".into()], "Single quote".into()),
            (vec!["web".into(), "both".into()], "Inline wins".into()),
            (vec!["web".into(), "empty".into()], "Keep above".into()),
            (
                vec!["web".into(), "nested".into(), "page".into()],
                "Nested page".into(),
            ),
            (vec!["task".into()], "Task hint".into()),
        ]);
        for text in [text.to_owned(), text.replace('\n', "\r\n")] {
            assert_eq!(yaml_hints(&text).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_invalid_shapes_duplicates_and_trailing_documents() {
        for (ext, text) in [
            ("json", "{\"x\":null}"),
            ("json", "[]"),
            ("json", "\"https://example.com\""),
            ("json", "{\"x\":\"a\",\"x\":\"b\"}"),
            ("json", "{} {}"),
            ("json", "{broken"),
            ("yaml", "x: 42"),
            ("yaml", "x: null"),
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
