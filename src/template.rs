use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expression {
    Path(Vec<String>),
    Lookup(Box<Expression>, Vec<Expression>),
}

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn expression(text: &str) -> Result<Expression> {
    fn parse(text: &str, depth: usize) -> Result<Expression> {
        ensure!(depth < 64, "Reference nesting exceeds 64 levels");
        let text = text.trim();
        let (path, suffix) = text.split_once('[').map_or((text, ""), |(a, b)| (a, b));
        let segments: Vec<String> = path.trim().split('.').map(str::to_owned).collect();
        ensure!(
            segments.iter().all(|part| !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))),
            "Invalid reference: {text}"
        );
        let base = Expression::Path(segments);
        if suffix.is_empty() {
            ensure!(!text.contains(['[', ']']), "Malformed lookup: {text}");
            return Ok(base);
        }
        ensure!(suffix.ends_with(']'), "Unclosed lookup: {text}");
        let inner = &suffix[..suffix.len() - 1];
        let mut selectors = Vec::new();
        let mut nesting = 0usize;
        let mut start = 0;
        for (index, ch) in inner.char_indices() {
            match ch {
                '[' => nesting += 1,
                ']' => {
                    ensure!(nesting > 0, "Unexpected closing bracket: {text}");
                    nesting -= 1;
                }
                ',' if nesting == 0 => {
                    selectors.push(parse(&inner[start..index], depth + 1)?);
                    start = index + 1;
                }
                _ => {}
            }
        }
        ensure!(nesting == 0, "Unclosed lookup: {text}");
        selectors.push(parse(&inner[start..], depth + 1)?);
        Ok(Expression::Lookup(Box::new(base), selectors))
    }
    parse(text, 0)
}

pub fn names(template: &str) -> Result<Vec<String>> {
    ensure!(!template.contains('\0'), "Values cannot contain NUL");
    let mut names = vec![];
    let mut rest = template;
    while let Some(start) = rest.find(['{', '}']) {
        ensure!(
            rest.as_bytes()[start] == b'{',
            "Unmatched closing brace in resource"
        );
        let end = rest[start + 1..]
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("Unclosed placeholder in resource"))?
            + start
            + 1;
        let name = &rest[start + 1..end];
        expression(name)?;
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
        let end = rest[start..].find('}').unwrap() + start;
        static_target.push_str(&rest[..start]);
        static_target.push_str("placeholder");
        rest = &rest[end + 1..];
    }
    static_target.push_str(rest);
    crate::resource::validate(&static_target)
}

/// Declarations cover each distinct placeholder; their order controls CLI arguments.
pub fn validate_arguments(template: &str, args: Option<&[crate::script::Argument]>) -> Result<()> {
    let names = names(template)?
        .into_iter()
        .filter(|name| valid_name(name))
        .collect::<Vec<_>>();
    crate::script::validate_arguments(args)?;
    if let Some(args) = args {
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
            "{a()}",
        ] {
            assert!(names(bad).is_err(), "{bad}");
        }
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
