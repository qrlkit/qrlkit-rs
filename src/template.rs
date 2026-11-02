use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expression {
    Path(Vec<String>),
    Lookup(Box<Expression>, Vec<Expression>),
    /// A fixed lookup key generated when binding a selected record's fields.
    Key(String),
    Call(Vec<String>, Vec<(String, CallArgument)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallArgument {
    Literal(String),
    Template(String),
}

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn expression(text: &str) -> Result<Expression> {
    parse(text, 0)
}

fn parse(text: &str, depth: usize) -> Result<Expression> {
    ensure!(depth < 64, "Reference nesting exceeds 64 levels");
    let text = text.trim();
    let split = text.find(['[', '(']);
    let (path, suffix) = split.map_or((text, ""), |index| text.split_at(index));
    let segments: Vec<String> = path.trim().split('.').map(str::to_owned).collect();
    ensure!(
        segments.iter().all(|part| !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))),
        "Invalid reference: {text}"
    );
    let base = Expression::Path(segments.clone());
    if suffix.is_empty() {
        return Ok(base);
    }
    let call = suffix.starts_with('(');
    let closing = if call { ')' } else { ']' };
    ensure!(suffix.ends_with(closing), "Unclosed reference: {text}");
    let inner = &suffix[1..suffix.len() - 1];
    let pieces = split_arguments(inner)?;
    if call {
        let mut arguments = Vec::new();
        for piece in pieces {
            let (name, value) = piece
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("Expected named argument: {piece}"))?;
            let name = name.trim();
            let value = value.trim();
            ensure!(valid_name(name), "Invalid argument name: {name}");
            ensure!(
                !arguments.iter().any(|(key, _)| key == name),
                "Duplicate argument: {name}"
            );
            ensure!(!value.is_empty(), "Missing value for argument: {name}");
            let value = if value.starts_with('"') {
                CallArgument::Literal(serde_json::from_str::<String>(value)?)
            } else if value.starts_with('\'') {
                ensure!(
                    value.len() >= 2 && value.ends_with('\''),
                    "Unclosed argument string"
                );
                CallArgument::Literal(value[1..value.len() - 1].to_owned())
            } else if value.contains(['{', '}']) {
                names_at_depth(value, depth + 1)?;
                CallArgument::Template(value.to_owned())
            } else {
                ensure!(
                    !value.contains(['(', ')', '[', ']'])
                        && !value.chars().any(char::is_whitespace),
                    "Quote literal argument values containing spaces or delimiters"
                );
                CallArgument::Literal(value.to_owned())
            };
            arguments.push((name.to_owned(), value));
        }
        return Ok(Expression::Call(segments, arguments));
    }
    ensure!(!pieces.is_empty(), "Empty lookup: {text}");
    let selectors = pieces
        .into_iter()
        .map(|piece| parse(piece, depth + 1))
        .collect::<Result<_>>()?;
    Ok(Expression::Lookup(Box::new(base), selectors))
}

/// Split delimiters only outside nested references and quoted literals.
fn split_arguments(text: &str) -> Result<Vec<&str>> {
    if text.trim().is_empty() {
        return Ok(vec![]);
    }
    let mut pieces = vec![];
    let mut start = 0;
    let mut stack = vec![];
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => stack.push(')'),
            '[' => stack.push(']'),
            '{' => stack.push('}'),
            ')' | ']' | '}' => ensure!(
                stack.pop() == Some(ch),
                "Mismatched delimiter in reference: {text}"
            ),
            ',' if stack.is_empty() => {
                pieces.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    ensure!(
        stack.is_empty() && quote.is_none(),
        "Unclosed delimiter in reference: {text}"
    );
    pieces.push(text[start..].trim());
    Ok(pieces)
}

/// Byte index of the closing brace for a placeholder starting at byte zero.
pub fn placeholder_end(text: &str) -> Result<usize> {
    let mut depth = 0;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    anyhow::bail!("Unclosed placeholder in resource")
}

pub fn names(template: &str) -> Result<Vec<String>> {
    names_at_depth(template, 0)
}

fn names_at_depth(template: &str, depth: usize) -> Result<Vec<String>> {
    ensure!(!template.contains('\0'), "Values cannot contain NUL");
    let mut names = vec![];
    let mut rest = template;
    while let Some(start) = rest.find(['{', '}']) {
        ensure!(
            rest.as_bytes()[start] == b'{',
            "Unmatched closing brace in resource"
        );
        let end = placeholder_end(&rest[start..])? + start;
        let name = &rest[start + 1..end];
        parse(name, depth)?;
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
        rest = &rest[end + 1..];
    }
    Ok(names)
}

/// Validate static target syntax without evaluating references or requiring paths to exist.
pub fn validate_target(target: &str) -> Result<()> {
    names(target)?;
    ensure!(
        !target.contains(['\n', '\r', '\0']),
        "Targets cannot contain newlines or NUL"
    );
    if target.starts_with('{') {
        return Ok(());
    }
    let mut static_target = String::new();
    let mut rest = target;
    while let Some(start) = rest.find('{') {
        let end = placeholder_end(&rest[start..])? + start;
        static_target.push_str(&rest[..start]);
        static_target.push_str("placeholder");
        rest = &rest[end + 1..];
    }
    static_target.push_str(rest);
    crate::resource::validate(&static_target)
}

/// Declarations cover each distinct placeholder; their order controls CLI arguments.
pub fn validate_arguments(template: &str, args: Option<&[crate::script::Argument]>) -> Result<()> {
    let placeholders = names(template)?;
    crate::script::validate_arguments(args)?;
    if let Some(args) = args {
        fn collect(
            expr: &Expression,
            args: &[crate::script::Argument],
            names: &mut std::collections::BTreeSet<String>,
        ) -> Result<()> {
            match expr {
                Expression::Path(path) => {
                    if args.iter().any(|arg| arg.name == path[0]) {
                        names.insert(path[0].clone());
                    }
                }
                Expression::Lookup(base, selectors) => {
                    collect(base, args, names)?;
                    for selector in selectors {
                        collect(selector, args, names)?;
                    }
                }
                Expression::Call(_, values) => {
                    for (_, value) in values {
                        if let CallArgument::Template(text) = value {
                            for name in crate::template::names(text)? {
                                collect(&expression(&name)?, args, names)?;
                            }
                        }
                    }
                }
                Expression::Key(_) => {}
            }
            Ok(())
        }
        let mut names = std::collections::BTreeSet::new();
        for name in placeholders {
            if valid_name(&name) {
                names.insert(name.clone());
            }
            collect(&expression(&name)?, args, &mut names)?;
        }
        ensure!(
            args.len() == names.len() && args.iter().all(|arg| names.contains(&arg.name)),
            "Resource args must declare each placeholder exactly once"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_support_qualified_paths_nested_and_multiple_selectors() {
        for name in [
            "projects.org",
            "projects.qrlkit-rs",
            "logs.hosts[projects.repos, shared.environments, shared.regions]",
            "logs.hosts[projects.aliases[projects.repos], shared.regions]",
        ] {
            expression(name).unwrap();
        }
        assert_eq!(
            names("{data.names}/{data.names}/{other}").unwrap(),
            ["data.names", "other"]
        );
    }

    #[test]
    fn malformed_expressions_and_templates_are_errors() {
        for bad in [
            "./{",
            "./}",
            "./{}",
            "./{bad name}",
            "./{{nested}}",
            "{a[]}",
            "{a[b,]}",
            "{a[b][c]}",
            "{a..b}",
            "{a(,)}",
        ] {
            assert!(names(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn calls_parse_literals_and_nested_argument_templates() {
        assert_eq!(
            expression(r#"example.templ(area=2, text="a,b={}\\\"", value={other(x={height})})"#)
                .unwrap(),
            Expression::Call(
                vec!["example".into(), "templ".into()],
                vec![
                    ("area".into(), CallArgument::Literal("2".into())),
                    ("text".into(), CallArgument::Literal("a,b={}\\\"".into())),
                    (
                        "value".into(),
                        CallArgument::Template("{other(x={height})}".into())
                    ),
                ]
            )
        );
        assert_eq!(
            names("{a(x={b(y={z})})}/{a(x={b(y={z})})}").unwrap(),
            ["a(x={b(y={z})})"]
        );
        for bad in [
            "{a(x=)}",
            "{a(x=1,x=2)}",
            "{a(x=1,)}",
            "{a(1)}",
            "{a(x={b)}",
            "{a(x=\"unclosed)}",
            "{a(x={b},y=[)}",
        ] {
            assert!(names(bad).is_err(), "{bad}");
        }
        let deep = format!("{}x{}", "{a(x=".repeat(65), ")}".repeat(65));
        assert!(names(&deep).unwrap_err().to_string().contains("nesting"));
    }

    #[test]
    fn static_target_validation_defers_composed_values_but_rejects_invalid_schemes() {
        validate_target("{repos.open}/pulls").unwrap();
        validate_target("https://{logs.hosts[projects.repos, shared.regions]}.example.com")
            .unwrap();
        assert!(validate_target("javascript:alert(1)").is_err());
        assert!(validate_target("https://example.com/\n").is_err());
    }
}
