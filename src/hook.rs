use anyhow::{Result, ensure};

// Replace only complete, unquoted words. Paths are supplied as arguments, never
// interpolated into shell code. Leave quoted strings and escaped words intact.
pub fn expand(hook: &str, placeholder: &str, replacement: &str) -> Result<String> {
    ensure!(
        !hook.contains(['\0', '\n', '\r']),
        "{placeholder} hook cannot contain NUL or newlines"
    );
    let mut output = String::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut found = false;
    let flush = |word: &mut String, output: &mut String, found: &mut bool| {
        if word == placeholder {
            output.push_str(replacement);
            *found = true;
        } else {
            output.push_str(word);
        }
        word.clear();
    };
    for ch in hook.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
        } else if ch == '\\' && quote != Some('\'') {
            word.push(ch);
            escaped = true;
        } else if let Some(delimiter) = quote {
            word.push(ch);
            if ch == delimiter {
                quote = None;
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            word.push(ch);
        } else if ch.is_whitespace() || ";&|()<>{}".contains(ch) {
            flush(&mut word, &mut output, &mut found);
            output.push(ch);
        } else {
            word.push(ch);
        }
    }
    ensure!(
        quote.is_none() && !escaped,
        "Unclosed quote or trailing escape in {placeholder} hook"
    );
    flush(&mut word, &mut output, &mut found);
    ensure!(
        found,
        "Hook needs a standalone unquoted {placeholder} argument"
    );
    Ok(output)
}
