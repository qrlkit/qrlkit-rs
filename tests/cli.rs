use std::{fs, path::Path, process::Command};

fn run(config: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .current_dir(config.parent().unwrap())
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", config.parent().unwrap())
        .arg("--config")
        .arg(config)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn import_reload_remove_and_failure_atomicity() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    // Seed browser configuration so this test never prompts or launches a browser.
    fs::write(
        &config,
        "version: 1\nbrowser:\n  name: Test\n  executable: unused\n  args: []\nsources: []\n",
    )
    .unwrap();
    let source = dir.path().join("team.toml");
    fs::write(&source, "[git]\nprs = 'https://old.test'").unwrap();
    let path = source.to_str().unwrap();
    assert!(run(&config, &["add", path]).status.success());
    let before = fs::read(&config).unwrap();
    assert!(!run(&config, &["add", path]).status.success());
    assert_eq!(before, fs::read(&config).unwrap());
    let list = run(&config, &["ls"]);
    assert!(String::from_utf8_lossy(&list.stdout).contains("[git]"));
    fs::write(&source, "[git]\nprs = 'https://new.test'").unwrap();
    assert!(run(&config, &["reload"]).status.success());
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("https://new.test")
    );
    let before = fs::read(&config).unwrap();
    fs::write(&source, "broken = [").unwrap();
    assert!(!run(&config, &["reload"]).status.success());
    assert_eq!(before, fs::read(&config).unwrap());
    fs::remove_file(&source).unwrap();
    assert!(!run(&config, &["reload"]).status.success());
    assert_eq!(before, fs::read(&config).unwrap());
    assert!(run(&config, &["rm", path]).status.success());
    assert!(String::from_utf8_lossy(&run(&config, &["ls"]).stdout).contains("No imports"));
}

#[test]
fn help_and_version_skip_browser_setup_and_noninteractive_prompt_fails_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    assert!(run(&config, &["--help"]).status.success());
    assert!(run(&config, &["--version"]).status.success());
    assert!(!config.exists());
    let output = run(&config, &["set-browser"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
    assert!(!config.exists());
}

#[test]
fn bare_command_without_imports_shows_help_without_browser_setup() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let output = run(&config, &[]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("No imports"));
    assert!(text.contains("Usage:"));
    assert!(!config.exists());
}

#[test]
fn nuke_resets_only_selected_state_and_handles_corruption_and_absence() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let other = dir.path().join("other.yaml");
    let source = dir.path().join("team.toml");
    fs::write(&source, "[prs]\nrepo = 'https://example.com'").unwrap();
    fs::write(&other, "unrelated state").unwrap();
    for content in [
        "version: 1\nbrowser: null\nsources: []\n",
        "invalid yaml: [",
    ] {
        fs::write(&config, content).unwrap();
        let result = run(&config, &["nuke"]);
        assert!(result.status.success(), "{:?}", result);
        assert!(!config.exists());
        assert!(source.exists());
        assert_eq!(fs::read_to_string(&other).unwrap(), "unrelated state");
        let fresh = run(&config, &[]);
        assert!(fresh.status.success());
        assert!(String::from_utf8_lossy(&fresh.stdout).contains("No imports"));
    }
    assert!(run(&config, &["nuke"]).status.success());
    fs::create_dir(&config).unwrap();
    assert!(!run(&config, &["nuke"]).status.success());
    assert!(config.is_dir());
}

#[test]
fn nuke_removes_managed_shell_integration_but_preserves_user_config() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    fs::write(&config, "version: 1\nbrowser: null\nsources: []\n").unwrap();
    let startup = dir.path().join(".zshrc");
    fs::write(
        &startup,
        "export KEEP_ME=yes\n# >>> QRL shell integration >>>\nold integration\n# <<< QRL shell integration <<<\n",
    )
    .unwrap();
    let output = run(&config, &["nuke"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(startup).unwrap(), "export KEEP_ME=yes\n");
}

fn seeded_config(dir: &Path) -> std::path::PathBuf {
    let config = dir.join("state.yaml");
    fs::write(
        &config,
        "version: 1\nbrowser: {name: Test, executable: missing-qrl-test-browser}\nsources: []\n",
    )
    .unwrap();
    config
}

#[test]
fn add_collision_and_reserved_roots_fail_without_a_terminal_and_preserve_state() {
    let dir = tempfile::tempdir().unwrap();
    let config = seeded_config(dir.path());
    let first = dir.path().join("first.toml");
    fs::write(&first, "[git]\nprs = 'https://example.com'").unwrap();
    assert!(
        run(&config, &["add", first.to_str().unwrap()])
            .status
            .success()
    );
    let before = fs::read(&config).unwrap();
    let second = dir.path().join("second.toml");
    for root in ["git", "nuke", "help", "add"] {
        fs::write(&second, format!("[{root}]\nother = 'https://example.com'")).unwrap();
        let result = run(&config, &["add", second.to_str().unwrap()]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("interactive terminal"));
        assert_eq!(fs::read(&config).unwrap(), before);
    }
}

#[test]
fn reload_is_atomic_across_multiple_files_and_new_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let config = seeded_config(dir.path());
    let a = dir.path().join("a.toml");
    let b = dir.path().join("b.toml");
    fs::write(&a, "[git]\nprs = 'https://old.test'").unwrap();
    fs::write(&b, "[logs]\napi = 'https://logs.test'").unwrap();
    for path in [&a, &b] {
        assert!(
            run(&config, &["add", path.to_str().unwrap()])
                .status
                .success()
        );
    }
    let before = fs::read(&config).unwrap();
    fs::write(&a, "[git]\nprs = 'https://new.test'").unwrap();
    for text in [
        "bad = [",
        "[git]\nissues = 'https://issues.test'",
        "[nuke]\napi = 'https://logs.test'",
    ] {
        fs::write(&b, text).unwrap();
        assert!(!run(&config, &["reload"]).status.success());
        assert_eq!(fs::read(&config).unwrap(), before);
    }
}

#[test]
fn lookup_distinguishes_unknown_partial_and_unlaunchable_paths() {
    let dir = tempfile::tempdir().unwrap();
    let config = seeded_config(dir.path());
    let source = dir.path().join("team.toml");
    fs::write(&source, "[gh.prs]\nrepo = 'https://example.com'").unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let before = fs::read(&config).unwrap();
    for (args, expected) in [
        (vec![], "interactive terminal"),
        (vec!["gh", "prs"], "interactive terminal"),
        (vec!["prs"], "No resource"),
        (vec!["gh", "prs", "repo"], "Cannot launch"),
        (vec!["gh", "prs", "repo", "extra"], "No resource"),
    ] {
        let result = run(&config, &args);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected),
            "{result:?}"
        );
        assert_eq!(fs::read(&config).unwrap(), before);
    }
}

#[test]
fn malformed_state_entries_are_errors_not_panics_and_nuke_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    for entries in [
        "[{key: [], url: 'https://example.com'}]",
        "[{key: ['bad key'], url: 'https://example.com'}]",
        "[{key: [git], url: 'javascript:alert(1)'}]",
        "[{key: [git], url: 'https://a.test'}, {key: [git], url: 'https://b.test'}]",
        "[{key: [git], url: 'https://a.test'}, {key: [git, prs], url: 'https://b.test'}]",
    ] {
        fs::write(&config, format!("version: 1\nbrowser: {{name: Test, executable: unused}}\nsources:\n  - path: team.toml\n    entries: {entries}\n")).unwrap();
        let before = fs::read(&config).unwrap();
        let result = run(&config, &["ls"]);
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
        assert_eq!(fs::read(&config).unwrap(), before);
        assert!(run(&config, &["nuke"]).status.success());
    }
}

#[cfg(unix)]
#[test]
fn symlink_import_is_deduplicated_and_nuking_state_link_preserves_target() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let config = seeded_config(dir.path());
    let source = dir.path().join("source.toml");
    let alias = dir.path().join("alias.toml");
    fs::write(&source, "[prs]\nrepo = 'https://example.com'").unwrap();
    symlink(&source, &alias).unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let before = fs::read(&config).unwrap();
    let result = run(&config, &["add", alias.to_str().unwrap()]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("already imported"));
    assert_eq!(fs::read(&config).unwrap(), before);
    let config_link = dir.path().join("state-link.yaml");
    symlink(&config, &config_link).unwrap();
    assert!(run(&config_link, &["nuke"]).status.success());
    assert!(!config_link.exists());
    assert_eq!(fs::read(&config).unwrap(), before);
}

#[test]
fn filesystem_resources_resolve_from_shell_and_never_run_files() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project with spaces");
    fs::create_dir(&project).unwrap();
    let file = project.join("script.sh");
    fs::write(&file, "exit 99\n").unwrap();
    let source = project.join("links.toml");
    fs::write(
        &source,
        "[work]\nfile = './project with spaces/script.sh'\ndir = './project with spaces'\nmissing = './not-here'\n",
    )
    .unwrap();
    let config = dir.path().join("state.yaml");
    let added = run(&config, &["add", source.to_str().unwrap()]);
    assert!(added.status.success(), "{added:?}");
    let result = run(&config, &["work", "file"]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!("{}\n", fs::canonicalize(&file).unwrap().display())
    );
    assert!(result.stderr.is_empty());
    let result = run(&config, &["work", "dir"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("shell integration"));
    let output_file = dir.path().join("cd-target");
    let result = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .current_dir(dir.path())
        .env("QRL_CD_FILE", &output_file)
        .arg("--config")
        .arg(&config)
        .args(["work", "dir"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(&output_file).unwrap(),
        format!("{}\n", fs::canonicalize(&project).unwrap().display())
    );
    assert!(!run(&config, &["work", "missing"]).status.success());
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("browser: null")
    );
}

#[test]
fn init_without_shell_reaches_browser_prompt_without_changing_state() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let result = run(&config, &["init"]);
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(error.contains("Choose the browser"), "{error}");
    assert!(error.contains("interactive terminal"), "{error}");
    assert!(!error.contains("required arguments"), "{error}");
    assert!(!config.exists());
}

#[test]
fn shell_initialization_does_not_require_state() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    fs::write(&config, "corrupt[").unwrap();
    for shell in ["bash", "zsh", "fish"] {
        let result = run(&config, &["init", shell]);
        assert!(result.status.success(), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains("QRL_CD_FILE"));
    }
    assert!(!run(&config, &["init", "unknown"]).status.success());
    assert_eq!(fs::read_to_string(config).unwrap(), "corrupt[");
}

#[cfg(unix)]
#[test]
fn shell_wrapper_changes_parent_directory_and_preserves_file_output() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project with spaces; literal");
    fs::create_dir(&project).unwrap();
    let file = project.join("test.sh");
    fs::write(&file, "do not execute").unwrap();
    let source = project.join("links.toml");
    fs::write(
        &source,
        "[work]\ndir = './project with spaces; literal'\nfile = './test.sh'\n",
    )
    .unwrap();
    let config = dir.path().join("state.yaml");
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_qrlkit")).parent().unwrap();
    let mut paths = vec![binary_dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    for shell in ["bash", "zsh"] {
        if Command::new(shell).arg("--version").output().is_err() {
            continue;
        }
        let init = run(&config, &["init", shell]);
        let script = format!(
            "{}\nqrlkit --config \"$QRL_TEST_CONFIG\" work dir || exit\npwd -P\nqrlkit --config \"$QRL_TEST_CONFIG\" work file\n",
            String::from_utf8(init.stdout).unwrap()
        );
        let result = Command::new(shell)
            .args(["-c", &script])
            .env("PATH", std::env::join_paths(&paths).unwrap())
            .env("QRL_TEST_CONFIG", &config)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(result.status.success(), "{shell}: {result:?}");
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!(
                "{}\n{}\n",
                fs::canonicalize(&project).unwrap().display(),
                fs::canonicalize(&file).unwrap().display()
            )
        );
    }
}

#[test]
fn script_import_reload_and_namespace_lookup_never_execute() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let source = dir.path().join("scripts.toml");
    fs::write(&source, "[gh.make-pr]\n\"$run\" = 'echo ran > marker.txt'\n[gh.repo]\nqrlkit = 'https://example.com'\n").unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    assert!(run(&config, &["reload"]).status.success());
    assert!(run(&config, &["ls"]).status.success());
    let result = run(&config, &["gh"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("interactive terminal"));
    assert!(!dir.path().join("marker.txt").exists());
}

#[test]
fn invalid_script_definitions_leave_state_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let config = seeded_config(dir.path());
    let before = fs::read(&config).unwrap();
    let source = dir.path().join("scripts.toml");
    for text in [
        "[task]\n\"$run\" = 42",
        "[task]\n\"$run\" = ''",
        "[task]\n\"$shell\" = 'bash'",
        "[task]\n\"$run\" = 'echo hi'\n\"$shell\" = 42",
        "[task]\n\"$run\" = 'echo hi'\n\"$shell\" = 'unknown'",
        "[task]\n\"$run\" = 'echo hi'\nchild = 'https://example.com'",
        "[task]\n\"$run\" = 'echo hi'\n[task.child]\nurl = 'https://example.com'",
        "\"$run\" = 'echo hi'",
    ] {
        fs::write(&source, text).unwrap();
        let result = run(&config, &["add", source.to_str().unwrap()]);
        assert!(!result.status.success(), "{text}");
        assert_eq!(fs::read(&config).unwrap(), before);
    }
}

#[cfg(unix)]
#[test]
fn scripts_use_source_directory_stream_output_stop_on_error_and_return_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let project = dir.path().join("project with spaces");
    fs::create_dir(&project).unwrap();
    fs::create_dir(project.join("subdir")).unwrap();
    let source = project.join("scripts.toml");
    fs::write(
        &source,
        r#"
[gh.make-pr]
"$shell" = "bash"
"$run" = '''
cd ./subdir
printf 'stdout-test\n'
printf 'stderr-test\n' >&2
pwd -P > ../marker.txt
exit 7
'''
[gh.fail]
"$run" = '''
false
printf 'should not run' > failed.txt
'''
[gh.pipeline]
"$run" = '''
false | true
printf 'should not run' > failed.txt
'''
"#,
    )
    .unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let result = run(&config, &["gh", "make-pr"]);
    assert_eq!(result.status.code(), Some(7));
    assert_eq!(result.stdout, b"stdout-test\n");
    assert_eq!(result.stderr, b"qrlkit: running script..\nstderr-test\n");
    assert_eq!(
        fs::read_to_string(project.join("marker.txt"))
            .unwrap()
            .trim(),
        fs::canonicalize(project.join("subdir"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    for key in ["fail", "pipeline"] {
        assert!(!run(&config, &["gh", key]).status.success());
        assert!(!project.join("failed.txt").exists());
    }
}

#[cfg(unix)]
#[test]
fn script_receives_stdin_but_not_outer_directory_channel() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let source = dir.path().join("script.toml");
    fs::write(&source, "[task]\n\"$run\" = '''test -z \"${QRL_CD_FILE+x}\"\nread -r line\nprintf '%s' \"$line\"'''\n").unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let channel = dir.path().join("channel");
    fs::write(&channel, "untouched").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .args(["--config", config.to_str().unwrap(), "task"])
        .env("QRL_CD_FILE", &channel)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"literal input\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"literal input");
    assert_eq!(fs::read_to_string(channel).unwrap(), "untouched");
}

#[test]
fn directory_import_sets_up_shell_once_and_reload_detects_new_directory() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let startup = dir.path().join(".zshrc");
    fs::write(&startup, "export KEEP_ME=yes\n").unwrap();
    let source = dir.path().join("links.toml");
    fs::write(&source, "[work]\nfuture = './future'\nfile = './links.toml'\nsite = 'https://example.com'\n[work.script]\n\"$run\" = 'echo test'\n").unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let initial = fs::read_to_string(&startup).unwrap();
    assert!(initial.starts_with("export KEEP_ME=yes\n"));
    assert!(initial.contains("qrlkit --config"));
    assert!(initial.contains("__aliases zsh"));
    fs::create_dir(dir.path().join("future")).unwrap();
    let result = run(&config, &["reload"]);
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stderr).contains("Open a new terminal"));
    let configured = fs::read(&startup).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&configured)
            .matches("# >>> QRL")
            .count(),
        1
    );
    assert!(run(&config, &["reload"]).status.success());
    assert_eq!(fs::read(&startup).unwrap(), configured);
    assert_eq!(
        fs::read_to_string(dir.path().join(".zshrc.qrl-backup")).unwrap(),
        "export KEEP_ME=yes\n"
    );
}

#[test]
fn automatic_reload_updates_paths_and_preserves_aliases_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let source = dir.path().join("source.toml");
    fs::write(dir.path().join("one"), "").unwrap();
    fs::write(dir.path().join("two"), "").unwrap();
    fs::write(&source, "[work]\nfile = './one'").unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let mut state: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    state["sources"][0]["renames"]["work"] = "team".into();
    state["sources"][0]["entries"][0]["key"][0] = "team".into();
    fs::write(&config, serde_yaml_ng::to_string(&state).unwrap()).unwrap();
    fs::write(&source, "[work]\nnew = './two'").unwrap();
    let result = run(&config, &["team", "new"]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        fs::canonicalize(dir.path().join("two"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(!run(&config, &["team", "file"]).status.success());
    let before = fs::read(&config).unwrap();
    for content in ["broken = [", "[work]\nnew = './two'\n[nuke]\nx = './one'"] {
        fs::write(&source, content).unwrap();
        assert!(!run(&config, &["team", "new"]).status.success());
        assert_eq!(fs::read(&config).unwrap(), before);
    }
    fs::remove_file(&source).unwrap();
    assert!(!run(&config, &["team", "new"]).status.success());
    assert!(
        run(&config, &["rm", source.to_str().unwrap()])
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn scripts_reload_before_execution_and_receive_literal_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let source = dir.path().join("source.toml");
    fs::write(&source, "[task]\n'$run' = 'echo stale'").unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    fs::write(&source, "[task]\n'$run' = '''printf '<%s>\\n' \"$@\"''' ").unwrap();
    for separator in [false, true] {
        let mut args = vec!["task"];
        if separator {
            args.push("--");
        }
        args.extend([
            "hello world",
            "",
            "$(touch injected)",
            "--help",
            "--config",
            "a'b",
            "line\nbreak",
        ]);
        let result = run(&config, &args);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            "<hello world>\n<>\n<$(touch injected)>\n<--help>\n<--config>\n<a'b>\n<line\nbreak>\n"
        );
        assert!(!dir.path().join("injected").exists());
    }
    fs::write(&source, "invalid = [").unwrap();
    let result = run(&config, &["task", "must not run"]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}

#[test]
fn directory_import_supports_mixed_formats_skips_existing_and_is_not_recursive() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let sources = dir.path().join("configs");
    fs::create_dir(&sources).unwrap();
    for (name, text) in [
        ("a.toml", "[docs]\nlink = 'https://example.com'"),
        ("b.YAML", "notes:\n  link: https://example.com"),
        ("c.yml", "projects:\n  link: https://example.com"),
        ("d.json", r#"{"bookmarks":{"link":"https://example.com"}}"#),
        ("ignore.txt", "not a config"),
    ] {
        fs::write(sources.join(name), text).unwrap();
    }
    fs::create_dir(sources.join("nested.json")).unwrap();
    fs::write(sources.join("nested.json/invalid.toml"), "broken = [").unwrap();
    let first = run(&config, &["add", sources.join("a.toml").to_str().unwrap()]);
    assert!(first.status.success(), "{first:?}");
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(stderr.contains("QRL aliases configured in"));
    assert!(stderr.contains("To activate tools start a new terminal or run:"));
    assert!(stderr.contains("exec zsh"));
    let result = run(&config, &["add", sources.to_str().unwrap()]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        String::from_utf8_lossy(&result.stdout)
            .matches("Imported ")
            .count(),
        3
    );
    let state: serde_json::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    let imported = state["sources"].as_array().unwrap();
    assert_eq!(imported.len(), 4);
    for (source, name) in imported.iter().zip(["a.toml", "b.YAML", "c.yml", "d.json"]) {
        assert!(source["path"].as_str().unwrap().ends_with(name));
    }
    let aliases = run(&config, &["__aliases", "bash"]);
    let aliases = String::from_utf8_lossy(&aliases.stdout);
    assert!(aliases.contains("--root 'docs'"));
    assert!(aliases.contains("--root 'notes'"));
    let before = fs::read(&config).unwrap();
    assert!(
        run(&config, &["add", sources.to_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(fs::read(&config).unwrap(), before);
}

#[test]
fn directory_import_failures_leave_state_and_shell_setup_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let sources = dir.path().join("configs");
    fs::create_dir(&sources).unwrap();
    let initial = dir.path().join("initial.toml");
    fs::write(&initial, "[existing]\nlink = 'https://example.com'").unwrap();
    assert!(
        run(&config, &["add", initial.to_str().unwrap()])
            .status
            .success()
    );
    let before = fs::read(&config).unwrap();
    let startup = fs::read(dir.path().join(".zshrc")).unwrap();
    let empty = run(&config, &["add", sources.to_str().unwrap()]);
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("No supported config files"));
    fs::write(
        sources.join("a.toml"),
        "[docs]\nlink = 'https://example.com'",
    )
    .unwrap();
    for bad in [
        "broken = [",
        "[docs]\nother = 'https://example.com'",
        "[existing]\nother = 'https://example.com'",
    ] {
        fs::write(sources.join("z.toml"), bad).unwrap();
        assert!(
            !run(&config, &["add", sources.to_str().unwrap()])
                .status
                .success()
        );
        assert_eq!(fs::read(&config).unwrap(), before);
        assert_eq!(fs::read(dir.path().join(".zshrc")).unwrap(), startup);
    }
}

#[cfg(unix)]
#[test]
fn directory_import_deduplicates_symlinked_files() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let sources = dir.path().join("configs");
    fs::create_dir(&sources).unwrap();
    let source = sources.join("a.toml");
    fs::write(&source, "[docs]\nlink = 'https://example.com'").unwrap();
    std::os::unix::fs::symlink(&source, sources.join("b.toml")).unwrap();
    let result = run(&config, &["add", sources.to_str().unwrap()]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        String::from_utf8_lossy(&result.stdout)
            .matches("Imported ")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn filehooks_persist_override_reload_and_preserve_literal_paths() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("state.yaml");
    let source = dir.path().join("files.toml");
    let filename = "a file;$(touch INJECTED).txt";
    fs::write(dir.path().join(filename), "contents").unwrap();
    let absolute = fs::canonicalize(dir.path().join(filename)).unwrap();
    fs::write(&source, format!("[notes]\ntoday = './{filename}'\n")).unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    let output = run(&config, &["notes", "today"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        absolute.to_str().unwrap()
    );

    assert!(
        run(&config, &["set-filehook", "printf 'global:%s' file"])
            .status
            .success()
    );
    let output = run(&config, &["notes", "today"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("global:{}", absolute.display())
    );
    assert!(!dir.path().join("INJECTED").exists());

    // Root default, group override, nested print override, and unaffected directory.
    fs::write(
        &source,
        format!(
            r#"
"$filehook" = "printf 'local:%s' file"
rootfile = './{filename}'
[notes]
"$filehook" = "printf 'group:%s' file"
today = './{filename}'
[notes.raw]
"$filehook" = ""
today = './{filename}'
[folders]
here = './'
"#
        ),
    )
    .unwrap();
    for (keys, expected) in [
        (vec!["rootfile"], format!("local:{}", absolute.display())),
        (
            vec!["notes", "today"],
            format!("group:{}", absolute.display()),
        ),
        (
            vec!["notes", "raw", "today"],
            format!("{}\n", absolute.display()),
        ),
    ] {
        let output = run(&config, &keys);
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
    let cd = dir.path().join("cd-target");
    let output = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .current_dir(dir.path())
        .env("QRL_CD_FILE", &cd)
        .args(["--config", config.to_str().unwrap(), "folders", "here"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(cd).unwrap().trim(),
        fs::canonicalize(dir.path()).unwrap().to_str().unwrap()
    );

    // Invalid commands must not overwrite the saved preference.
    let before = fs::read(&config).unwrap();
    assert!(!run(&config, &["set-filehook", "nvim"]).status.success());
    assert_eq!(fs::read(&config).unwrap(), before);
    assert!(run(&config, &["set-filehook", "--clear"]).status.success());
    fs::write(&source, format!("[notes]\ntoday = './{filename}'\n")).unwrap();
    assert_eq!(
        run(&config, &["notes", "today"]).stdout,
        format!("{}\n", absolute.display()).as_bytes()
    );

    // Propagate hook exit codes and report unavailable programs.
    assert!(
        run(&config, &["set-filehook", "sh -c 'exit 23' file"])
            .status
            .success()
    );
    assert_eq!(run(&config, &["notes", "today"]).status.code(), Some(23));
    assert!(
        run(&config, &["set-filehook", "qrl-nonexistent-editor file"])
            .status
            .success()
    );
    let output = run(&config, &["notes", "today"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("qrl-nonexistent-editor"));
}

#[cfg(unix)]
#[test]
fn directory_hooks_update_parent_shell_and_preserve_failure_status() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("state.yaml");
    let source = temp.path().join("dirs.toml");
    let name = "project space;$(touch INJECTED)'quote";
    let project = temp.path().join(name);
    fs::create_dir(&project).unwrap();
    let canonical = fs::canonicalize(&project).unwrap();
    // Double-quoted TOML keeps the literal single quote in the directory name.
    fs::write(&source, format!("[work]\nrepo = \"./{name}\"\n")).unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        run(
            &config,
            &["set-dirhook", "cd dir && printf 'hook:' && pwd -P"]
        )
        .status
        .success()
    );
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_qrlkit")).parent().unwrap();
    let mut paths = vec![binary_dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let invoke = |shell: &str| {
        let init = run(&config, &["init", shell]);
        Command::new(shell).args(["-c", &format!(
            "{}\nqrlkit --config \"$QRL_TEST_CONFIG\" work repo\nresult=$?\nprintf 'status:%s\\nparent:' \"$result\"\npwd -P",
            String::from_utf8(init.stdout).unwrap()
        )]).env("PATH", std::env::join_paths(&paths).unwrap())
            .env("QRL_TEST_CONFIG", &config)
            .env("SHELL", "/bin/zsh")
            .env("ZDOTDIR", temp.path()).current_dir(temp.path()).output().unwrap()
    };
    for shell in ["bash", "zsh"] {
        if Command::new(shell).arg("--version").output().is_err() {
            continue;
        }
        let output = invoke(shell);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!(
                "hook:{}\nstatus:0\nparent:{}\n",
                canonical.display(),
                canonical.display()
            )
        );
    }
    assert!(!temp.path().join("INJECTED").exists());
    // Auto-reloaded local default and narrower override take precedence.
    fs::write(&source, format!("\"$dirhook\" = \"cd dir && printf root\"\n[work]\n\"$dirhook\" = \"cd dir && exit 23\"\nrepo = \"./{name}\"\n")).unwrap();
    for shell in ["bash", "zsh"] {
        if Command::new(shell).arg("--version").output().is_err() {
            continue;
        }
        let output = invoke(shell);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("status:23\nparent:{}\n", canonical.display())
        );
    }
    fs::write(
        &source,
        format!("[work]\n\"$dirhook\" = \"\"\nrepo = \"./{name}\"\n"),
    )
    .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&invoke("bash").stdout),
        format!("status:0\nparent:{}\n", canonical.display())
    );
    let before = fs::read(&config).unwrap();
    assert!(!run(&config, &["set-dirhook", "cd"]).status.success());
    assert_eq!(fs::read(&config).unwrap(), before);
    assert!(run(&config, &["set-dirhook", "--clear"]).status.success());
    fs::write(&source, format!("[work]\nrepo = \"./{name}\"\n")).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&invoke("bash").stdout),
        format!("status:0\nparent:{}\n", canonical.display())
    );
}

#[cfg(unix)]
#[test]
fn filehooks_execute_shell_chains_pipes_redirects_and_local_overrides() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("state.yaml");
    let source = temp.path().join("files.toml");
    let filename = "file space'quote;$(touch INJECTED).txt";
    fs::write(temp.path().join(filename), "hello\n").unwrap();
    let resource = format!("[notes]\nitem = \"./{filename}\"\n");
    fs::write(&source, &resource).unwrap();
    assert!(
        run(&config, &["add", source.to_str().unwrap()])
            .status
            .success()
    );
    for (hook, status, expected) in [
        ("cat file && printf done", 0, "hello\ndone"),
        (
            "cat<file|tr a-z A-Z > result.txt && cat result.txt",
            0,
            "HELLO\n",
        ),
        (
            "test -z \"${QRL_CD_FILE+x}\" && cat file; printf '%s' \"$(printf substituted)\"",
            0,
            "hello\nsubstituted",
        ),
        ("cat file && false && printf skipped", 1, "hello\n"),
        ("false || cat file", 0, "hello\n"),
        ("cat file && sh -c 'exit 23'", 23, "hello\n"),
        ("cat file | sh -c 'cat >/dev/null; exit 17'", 17, ""),
    ] {
        assert!(run(&config, &["set-filehook", hook]).status.success());
        let output = run(&config, &["notes", "item"]);
        assert_eq!(output.status.code(), Some(status), "{hook}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{hook}");
    }
    assert!(!temp.path().join("INJECTED").exists());
    // A local hook also supports shell syntax and reloads without re-importing.
    fs::write(
        &source,
        format!("\"$filehook\" = \"cat file && printf local\"\n{resource}"),
    )
    .unwrap();
    let output = run(&config, &["notes", "item"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"hello\nlocal");
}
