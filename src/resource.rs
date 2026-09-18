use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};

pub fn is_url(value: &str) -> bool {
    value.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")
    })
}

pub fn validate(value: &str) -> Result<()> {
    if is_url(value) {
        return crate::import::validate_url(value);
    }
    ensure!(
        !value.contains(['\n', '\r', '\0']),
        "Paths cannot contain newlines or NUL"
    );
    ensure!(
        value == "~"
            || value.starts_with("~/")
            || value.starts_with("./")
            || value.starts_with("../")
            || Path::new(value).is_absolute(),
        "Expected an HTTP(S) URL or explicit path (~/, /, ./, ../, or a Windows absolute path): {value}"
    );
    Ok(())
}

/// Expand home paths while keeping relative paths for resolution at invocation.
/// Existence is checked on selection so imports remain portable and reloadable.
pub fn normalize(value: &str) -> Result<String> {
    validate(value)?;
    if is_url(value) {
        return Ok(value.into());
    }
    let path = if value == "~" || value.starts_with("~/") {
        let home = dirs::home_dir().context("Cannot locate home directory")?;
        home.join(value.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(value)
    };
    path.to_str()
        .map(str::to_owned)
        .context("Resource path is not valid UTF-8")
}

pub enum Resource {
    Url(String),
    Directory(PathBuf),
    File(PathBuf),
}

pub fn resolve(value: &str) -> Result<Resource> {
    validate(value)?;
    if is_url(value) {
        return Ok(Resource::Url(value.into()));
    }
    let path = std::fs::canonicalize(value)
        .with_context(|| format!("Resource path does not exist or cannot be accessed: {value}"))?;
    ensure!(
        path.to_str().is_some_and(|s| !s.contains(['\n', '\r'])),
        "Unsupported resource path encoding or newline"
    );
    if path.is_dir() {
        Ok(Resource::Directory(path))
    } else if path.is_file() {
        Ok(Resource::File(path))
    } else {
        bail!(
            "Resource is not a regular file or directory: {}",
            path.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn home_and_relative_paths_expand_without_requiring_existence() {
        assert_eq!(normalize("./not-created").unwrap(), "./not-created");
        assert_eq!(
            normalize("~/not-created").unwrap(),
            dirs::home_dir()
                .unwrap()
                .join("not-created")
                .to_str()
                .unwrap()
        );
        assert!(matches!(
            resolve("HTTPS://example.com").unwrap(),
            Resource::Url(_)
        ));
        for bad in [
            "script.sh",
            "ftp://example.com",
            "./bad\nname",
            "./bad\rname",
        ] {
            assert!(validate(bad).is_err());
        }
    }
}
