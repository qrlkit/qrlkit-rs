//! Behavioral contract for packaged themes, personal imports, and tool policy.
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const PACKAGED: [&str; 8] = [
    "terminal",
    "dracula",
    "catppuccin-mocha",
    "catppuccin-latte",
    "tokyo-night",
    "gruvbox-dark",
    "gruvbox-light",
    "nord",
];

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .current_dir(root)
        .env("HOME", root)
        .env("ZDOTDIR", root)
        .env("XDG_CONFIG_HOME", root)
        .env("SHELL", "/bin/zsh")
        .args(["--config", root.join("state.yaml").to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}

fn success(root: &Path, args: &[&str]) -> String {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn personal(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join("personal.toml");
    fs::write(
        &path,
        format!("[theme]\nname = '{name}'\nselected = '#123456'\nmarker = '--x'\n"),
    )
    .unwrap();
    path
}

#[test]
fn catalog_and_current_are_read_only_and_removed_names_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let list = success(root, &["themes"]);
    for name in PACKAGED {
        assert!(
            list.lines().any(|line| line.trim_start().starts_with(name)),
            "{name}: {list}"
        );
    }
    assert_eq!(list.matches("Open repository").count(), PACKAGED.len());
    assert!(!list.contains('\u{1b}'));
    let current = success(root, &["theme", "current"]);
    assert!(current.contains("Global theme: terminal"));
    assert!(current.contains("Tool themes: on"));
    assert!(current.contains("Denied tools: none"));
    for name in ["ocean", "forest", "rose", "unknown"] {
        assert!(!run(root, &["themes", "set", name]).status.success());
        assert!(!list.lines().any(|line| line.trim_start().starts_with(name)));
    }
    assert!(!run(root, &["themes", "set"]).status.success());
    assert!(!root.join("state.yaml").exists());
}

#[test]
fn terminal_is_listed_first_before_alphabetical_packaged_and_imported_themes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = personal(root, "aardvark");
    success(root, &["themes", "add", path.to_str().unwrap()]);
    success(root, &["themes", "set", "dracula"]);
    for command in ["theme", "themes"] {
        let list = success(root, &[command]);
        assert_eq!(list.lines().nth(1), Some("  terminal"));
        let mut position = 0;
        for name in [
            "terminal",
            "aardvark",
            "catppuccin-latte",
            "catppuccin-mocha",
            "dracula",
            "gruvbox-dark",
            "gruvbox-light",
            "nord",
            "tokyo-night",
        ] {
            let heading = format!(
                "  {name}{}\n",
                if name == "dracula" { " (default)" } else { "" }
            );
            let found = list.find(&heading).unwrap();
            assert!(found > position, "Theme out of order: {name}");
            position = found;
        }
    }
}

#[test]
fn imports_are_named_copies_and_only_activate_when_selected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = personal(root, "personal");
    success(root, &["themes", "add", path.to_str().unwrap()]);
    fs::remove_file(path).unwrap();
    assert!(success(root, &["themes"]).contains("personal"));
    assert!(success(root, &["themes", "current"]).contains("Global theme: terminal"));
    success(root, &["theme", "set", "personal"]);
    assert!(success(root, &["themes", "current"]).contains("Global theme: personal"));
    let before = fs::read(root.join("state.yaml")).unwrap();
    success(root, &["themes"]);
    success(root, &["themes", "current"]);
    assert_eq!(fs::read(root.join("state.yaml")).unwrap(), before);
}

#[test]
fn invalid_imports_duplicates_and_unknown_selections_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = personal(root, "personal");
    success(root, &["themes", "add", path.to_str().unwrap()]);
    let before = fs::read(root.join("state.yaml")).unwrap();
    for text in [
        "[theme]\nselected = '#112233'",
        "name = 'wrong-place'\n[theme]\nselected = '#112233'",
        "[theme]\nname = ''",
        "[theme]\nname = 'two words'",
        "[theme]\nname = 'personal'",
        "[theme]\nname = 'dracula'",
        "[theme]\nname = 'bad'\nbackground = 'purple'",
        "[theme]\nname = 'bad'\nunknown = 'value'",
        "[theme]\nname = 'bad'\nselection_width = 'tiny'",
        "[theme]\nname = 'bad'\nborder = 'wavy'",
        "[theme]\nname = 'bad'\nmarker = \"a\\nb\"",
        "[theme]\nname = 'bad'\nselected = '#abcdef'\nselected = '#123456'",
    ] {
        fs::write(&path, text).unwrap();
        let output = run(root, &["themes", "add", path.to_str().unwrap()]);
        assert!(!output.status.success(), "Accepted {text}");
        assert!(!output.stderr.is_empty());
        assert_eq!(fs::read(root.join("state.yaml")).unwrap(), before);
    }
    for args in [vec!["themes", "set", "unknown"], vec!["themes", "set"]] {
        assert!(!run(root, &args).status.success());
        assert_eq!(fs::read(root.join("state.yaml")).unwrap(), before);
    }
}

#[test]
fn tool_policy_keeps_exclusions_while_toggled_and_reset_removes_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    success(root, &["themes", "tools", "deny", "gh"]);
    success(root, &["themes", "tools", "deny", "qk"]);
    success(root, &["themes", "tools", "deny", "gh"]); // Idempotent
    for mode in ["off", "on"] {
        success(root, &["themes", "tools", mode]);
        let current = success(root, &["themes", "current"]);
        assert!(current.contains(&format!("Tool themes: {mode}")));
        assert!(current.contains("Denied tools: gh, qk"));
    }
    success(root, &["themes", "tools", "reset", "gh"]);
    assert!(success(root, &["themes", "current"]).contains("Denied tools: qk"));
    success(root, &["themes", "tools", "reset", "qk"]);
    assert!(success(root, &["themes", "current"]).contains("Denied tools: none"));
    let before = fs::read(root.join("state.yaml")).unwrap();
    assert!(
        !run(root, &["themes", "tools", "deny", "two words"])
            .status
            .success()
    );
    assert_eq!(fs::read(root.join("state.yaml")).unwrap(), before);
}

#[test]
fn nuke_resets_global_theme_imports_and_policy_and_can_start_again() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = personal(root, "personal");
    success(root, &["themes", "add", path.to_str().unwrap()]);
    success(root, &["themes", "set", "personal"]);
    success(root, &["themes", "tools", "deny", "gh"]);
    success(root, &["themes", "tools", "off"]);
    success(root, &["nuke"]);
    assert!(!root.join("state.yaml").exists());
    assert!(!success(root, &["themes"]).contains("personal"));
    let current = success(root, &["themes", "current"]);
    assert!(current.contains("Global theme: terminal"));
    assert!(current.contains("Tool themes: on"));
    assert!(current.contains("Denied tools: none"));
    let target = root.join("repo.txt");
    fs::write(&target, "hello").unwrap();
    let resources = root.join("resources.toml");
    fs::write(&resources, format!("[gh]\nrepo = '{}'", target.display())).unwrap();
    success(root, &["add", resources.to_str().unwrap()]);
    assert_eq!(
        success(root, &["gh", "repo"]).trim(),
        target.canonicalize().unwrap().to_str().unwrap()
    );
}

#[test]
fn packaged_files_define_complete_named_themes() {
    let mut names = std::collections::BTreeSet::new();
    for name in PACKAGED {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("themes")
            .join(format!("{name}.toml"));
        let text = fs::read_to_string(path).unwrap();
        let doc: toml::Value = toml::from_str(&text).unwrap();
        let theme = doc["theme"].as_table().unwrap();
        assert_eq!(theme["name"].as_str(), Some(name));
        assert!(names.insert(name));
        for field in [
            "background",
            "text",
            "selected",
            "selected_background",
            "hint",
            "title",
            "border",
            "border_color",
            "marker",
            "selection_width",
        ] {
            assert!(theme.contains_key(field), "{name} is missing {field}");
        }
    }
}
