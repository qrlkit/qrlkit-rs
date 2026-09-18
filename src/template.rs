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
