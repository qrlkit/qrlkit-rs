use anyhow::{Result, ensure};

pub fn names(template: &str) -> Result<Vec<String>> {
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
        ensure!(
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "Placeholder names must contain letters, digits or underscores"
        );
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
        rest = &rest[end + 1..];
    }
    Ok(names)
}

pub fn expand(
    template: &str,
    args: &[String],
    mut prompt: impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let names = names(template)?;
    ensure!(
        args.len() <= names.len(),
        "Expected {} argument(s), got {}",
        names.len(),
        args.len()
    );
    let mut values = args.to_vec();
    for name in names.iter().skip(values.len()) {
        values.push(prompt(name)?);
    }
    let url = crate::resource::is_url(template);
    let mut result = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let end = rest[start..].find('}').unwrap() + start;
        result.push_str(&rest[..start]);
        let index = names
            .iter()
            .position(|n| n == &rest[start + 1..end])
            .unwrap();
        let value = &values[index];
        if url {
            for byte in value.bytes() {
                if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                    result.push(char::from(byte));
                } else {
                    use std::fmt::Write;
                    write!(result, "%{byte:02X}")?;
                }
            }
        } else {
            result.push_str(value);
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordering_reuse_encoding_and_literal_substitution() {
        let result = expand(
            "https://example.com/{env}/{id}?again={id}",
            &["prod".into(), "a /?&ø".into()],
            |_| panic!(),
        )
        .unwrap();
        assert_eq!(
            result,
            "https://example.com/prod/a%20%2F%3F%26%C3%B8?again=a%20%2F%3F%26%C3%B8"
        );
        assert_eq!(
            expand("./{one}/{two}", &["{two}".into()], |name| {
                assert_eq!(name, "two");
                Ok("literal $HOME".into())
            })
            .unwrap(),
            "./{two}/literal $HOME"
        );
    }
    #[test]
    fn malformed_extra_and_missing_arguments() {
        for bad in ["./{", "./}", "./{}", "./{bad name}", "./{{nested}}"] {
            assert!(names(bad).is_err());
        }
        assert!(expand("./fixed", &["extra".into()], |_| panic!()).is_err());
        assert!(expand("./{missing}", &[], |_| anyhow::bail!("cancelled")).is_err());
    }
}

/// Declarations cover each distinct placeholder; their order controls CLI arguments.
pub fn validate_arguments(template: &str, args: Option<&[crate::script::Argument]>) -> Result<()> {
    let names = names(template)?;
    crate::script::validate_arguments(args)?;
    if let Some(args) = args {
        ensure!(
            args.len() == names.len() && args.iter().all(|arg| names.contains(&arg.name)),
            "Resource args must declare each placeholder exactly once"
        );
    }
    Ok(())
}

pub fn expand_arguments(
    template: &str,
    args: Option<&[crate::script::Argument]>,
    supplied: &[String],
    mut prompt: impl FnMut(&crate::script::Argument) -> Result<String>,
) -> Result<String> {
    validate_arguments(template, args)?;
    let Some(args) = args else {
        return expand(template, supplied, |name| {
            prompt(&crate::script::Argument {
                name: name.to_owned(),
                choices: None,
            })
        });
    };
    let values = crate::script::resolve_arguments(Some(args), supplied, prompt)?;
    let ordered = names(template)?
        .iter()
        .map(|name| values[args.iter().position(|arg| &arg.name == name).unwrap()].clone())
        .collect::<Vec<_>>();
    expand(template, &ordered, |_| unreachable!())
}

#[cfg(test)]
mod argument_tests {
    use super::*;
    use crate::script::Argument;

    #[test]
    fn names_map_values_and_repeated_url_placeholders_encode_once() {
        let args = [
            Argument {
                name: "query".into(),
                choices: None,
            },
            Argument {
                name: "repo".into(),
                choices: Some(vec!["a /?&ø".into()]),
            },
        ];
        let result = expand_arguments(
            "https://example.com/{repo}?q={query}&again={repo}",
            Some(&args),
            &["hello world".into()],
            |arg| {
                assert_eq!(arg.name, "repo");
                Ok(arg.choices.as_ref().unwrap()[0].clone())
            },
        )
        .unwrap();
        assert_eq!(
            result,
            "https://example.com/a%20%2F%3F%26%C3%B8?q=hello%20world&again=a%20%2F%3F%26%C3%B8"
        );
        assert!(
            expand_arguments(
                "./{repo}/{query}",
                Some(&args),
                &["x".into(), "bad".into()],
                |_| panic!("invalid supplied values must not prompt")
            )
            .is_err()
        );
        assert!(
            expand_arguments("./{repo}/{query}", Some(&args), &["x".into()], |_| Ok(
                "bad".into()
            ))
            .is_err()
        );
        assert_eq!(
            expand_arguments("./fixed", Some(&[]), &[], |_| panic!()).unwrap(),
            "./fixed"
        );
    }
}
