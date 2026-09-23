//! Run the same public CLI contract against every import adapter.
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    dir: tempfile::TempDir,
    source: PathBuf,
    config: PathBuf,
    extension: &'static str,
}
impl Fixture {
    fn new(extension: &'static str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join(format!("resources.{extension}"));
        let config = dir.path().join("state.yaml");
        Self {
            dir,
            source,
            config,
            extension,
        }
    }
    fn write(&self, value: Value) {
        let text = match self.extension {
            "toml" => ::toml::to_string(&value).unwrap(),
            "json" => serde_json::to_string_pretty(&value).unwrap(),
            _ => serde_yaml_ng::to_string(&value).unwrap(),
        };
        fs::write(&self.source, text).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_qrlkit"))
            .current_dir(self.dir.path())
            .env("SHELL", "/bin/zsh")
            .env("ZDOTDIR", self.dir.path())
            .arg("--config")
            .arg(&self.config)
            .args(args)
            .output()
            .unwrap()
    }
    fn add(&self) {
        success(self.run(&["add", self.source.to_str().unwrap()]));
    }
    fn state(&self) -> Value {
        serde_yaml_ng::from_str(&fs::read_to_string(&self.config).unwrap()).unwrap()
    }
}
fn success(output: Output) -> Output {
    assert!(output.status.success(), "{output:?}");
    output
}
fn path_output(output: Output, path: &Path) {
    assert_eq!(
        String::from_utf8(success(output).stdout).unwrap().trim(),
        fs::canonicalize(path).unwrap().to_str().unwrap()
    );
}

fn lifecycle(ext: &'static str) {
    let f = Fixture::new(ext);
    fs::write(f.dir.path().join("first"), "").unwrap();
    fs::write(f.dir.path().join("second"), "").unwrap();
    f.write(json!({"files":{"old":"./first"},"web":{"logs":"https://example.com/?x=1&y=2"}}));
    f.add();
    let before = fs::read(&f.config).unwrap();
    assert!(!f.run(&["add", f.source.to_str().unwrap()]).status.success());
    assert_eq!(before, fs::read(&f.config).unwrap());
    assert!(
        String::from_utf8(success(f.run(&["ls"])).stdout)
            .unwrap()
            .contains(f.source.file_name().unwrap().to_str().unwrap())
    );
    path_output(f.run(&["files", "old"]), &f.dir.path().join("first"));
    assert!(
        f.state()["sources"][0]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["url"] == "https://example.com/?x=1&y=2")
    );
    f.write(json!({"files":{"new":"./second"}}));
    path_output(f.run(&["files", "new"]), &f.dir.path().join("second"));
    assert!(!f.run(&["files", "old"]).status.success());
    success(f.run(&["reload"]));
    let before = fs::read(&f.config).unwrap();
    fs::write(&f.source, "[broken").unwrap();
    assert!(!f.run(&["files", "new"]).status.success());
    assert_eq!(before, fs::read(&f.config).unwrap());
    fs::remove_file(&f.source).unwrap();
    assert!(!f.run(&["reload"]).status.success());
    success(f.run(&["rm", f.source.to_str().unwrap()]));
    assert!(f.state()["sources"].as_array().unwrap().is_empty());
}

fn invalid_resources(ext: &'static str) {
    let f = Fixture::new(ext);
    f.write(json!({"web":"https://example.com"}));
    f.add();
    let before = fs::read(&f.config).unwrap();
    for value in [
        json!({}),
        json!({"x":true}),
        json!({"x":[1,2]}),
        json!({"x":42}),
        json!({"bad key":"https://example.com"}),
        json!({"x":"javascript:alert(1)"}),
        json!({"x":"relative/path"}),
        json!({"x":{"$run":""}}),
        json!({"x":{"$shell":"bash"}}),
        json!({"x":{"$run":"echo hi","child":"https://example.com"}}),
        json!({"x":{"$run":"echo hi","$shell":"unknown"}}),
    ] {
        f.write(value.clone());
        let result = f.run(&["reload"]);
        assert!(!result.status.success(), "{ext}: {value}: {result:?}");
        assert_eq!(before, fs::read(&f.config).unwrap());
    }
}

fn directory_and_literal_keys(ext: &'static str) {
    let f = Fixture::new(ext);
    let target = f.dir.path().join("space and ø; literal");
    fs::create_dir(&target).unwrap();
    f.write(json!({"paths":{"api.v2":"./space and ø; literal"}}));
    f.add();
    let channel = f.dir.path().join("channel");
    let result = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .current_dir(f.dir.path())
        .env("QRL_CD_FILE", &channel)
        .arg("--config")
        .arg(&f.config)
        .args(["paths", "api.v2"])
        .output()
        .unwrap();
    let output = success(result);
    assert!(output.stdout.is_empty());
    let expected = fs::canonicalize(&target).unwrap();
    assert_eq!(
        fs::read_to_string(channel).unwrap(),
        format!("{}\n", expected.display())
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("qrlkit: goto {}\n", expected.display())
    );
}

fn collisions(ext: &'static str) {
    let f = Fixture::new(ext);
    f.write(json!({"web":{"a":"https://a.test"}}));
    f.add();
    let other = f.dir.path().join("other.toml");
    fs::write(&other, "[web]\nb = 'https://b.test'").unwrap();
    let before = fs::read(&f.config).unwrap();
    assert!(!f.run(&["add", other.to_str().unwrap()]).status.success());
    assert_eq!(before, fs::read(&f.config).unwrap());
    let mut state = f.state();
    state["sources"][0]["renames"]["web"] = json!("team-web");
    state["sources"][0]["entries"][0]["key"][0] = json!("team-web");
    fs::write(&f.config, serde_yaml_ng::to_string(&state).unwrap()).unwrap();
    f.write(json!({"web":{"new":"https://new.test"}}));
    success(f.run(&["reload"]));
    assert_eq!(
        f.state()["sources"][0]["entries"][0]["key"],
        json!(["team-web", "new"])
    );
    let before = fs::read(&f.config).unwrap();
    f.write(json!({"nuke":"https://example.com"}));
    assert!(!f.run(&["reload"]).status.success());
    assert_eq!(before, fs::read(&f.config).unwrap());
}

fn scripts(ext: &'static str) {
    let f = Fixture::new(ext);
    let shell = "bash";
    let body = "pwd -P\nprintf '<%s>\\n' \"$@\"\nexit 7";
    let updated = "printf 'updated\\n'";
    f.write(json!({"tasks":{"nested":{"$run":body,"$shell":shell}}}));
    f.add();
    success(f.run(&["reload"]));
    assert!(!f.run(&["tasks"]).status.success()); // Browsing never auto-runs a child.
    for separator in [false, true] {
        let mut args = vec!["tasks", "nested"];
        if separator {
            args.push("--");
        }
        args.extend(["first value", "--title", "ø third"]);
        let result = f.run(&args);
        assert_eq!(result.status.code(), Some(7), "{result:?}");
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!(
                "{}\n<first value>\n<--title>\n<ø third>\n",
                fs::canonicalize(f.dir.path()).unwrap().display()
            )
        );
    }
    f.write(json!({"tasks":{"nested":{"$run":updated,"$shell":shell}}}));
    assert_eq!(
        String::from_utf8(success(f.run(&["tasks", "nested"])).stdout)
            .unwrap()
            .trim(),
        "updated"
    );
}

fn paths_follow_invocation_directory(ext: &'static str) {
    let f = Fixture::new(ext);
    let first = f.dir.path().join("first");
    let second = f.dir.path().join("second");
    for directory in [&first, &second] {
        fs::create_dir(directory).unwrap();
        fs::write(directory.join("local.txt"), "").unwrap();
    }
    let fixed = f.dir.path().join("fixed.txt");
    fs::write(&fixed, "").unwrap();
    f.write(json!({"files":{"local":"./local.txt", "parent":"../fixed.txt", "fixed": fixed.to_str().unwrap()}}));
    f.add();
    for directory in [&first, &second] {
        for (key, expected) in [
            ("local", directory.join("local.txt")),
            ("parent", fixed.clone()),
            ("fixed", fixed.clone()),
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
                .current_dir(directory)
                .arg("--config")
                .arg(&f.config)
                .args(["files", key])
                .output()
                .unwrap();
            path_output(output, &expected);
        }
    }
    assert!(
        f.state()["sources"][0]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["url"] == "./local.txt")
    );
}

macro_rules! format_suite {
    ($name:ident, $ext:literal) => {
        mod $name {
            #[test]
            fn paths_follow_shell() {
                super::paths_follow_invocation_directory($ext);
            }
            #[test]
            fn import_lookup_reload_remove() {
                super::lifecycle($ext);
            }
            #[test]
            fn invalid_resources_preserve_state() {
                super::invalid_resources($ext);
            }
            #[test]
            fn directories_and_literal_keys() {
                super::directory_and_literal_keys($ext);
            }
            #[test]
            fn collisions_and_saved_renames() {
                super::collisions($ext);
            }
            #[test]
            fn nested_scripts_arguments_and_reload() {
                super::scripts($ext);
            }
        }
    };
}
format_suite!(toml, "toml");
format_suite!(yaml, "yaml");
format_suite!(yml, "yml");
format_suite!(json, "json");

#[test]
fn yaml_literal_and_folded_multiline_scripts_preserve_semantics() {
    let f = Fixture::new("yaml");
    fs::write(&f.source, "scripts:\n  literal:\n    $run: |\n      echo first\n      echo second\n  folded:\n    $run: >-\n      echo first\n      second\n").unwrap();
    f.add();
    let entries = f.state()["sources"][0]["entries"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(entries[0]["script"]["body"], "echo first second");
    assert_eq!(entries[1]["script"]["body"], "echo first\necho second\n");
}

#[test]
fn json_escapes_unicode_and_multiline_script_survive_import() {
    let f = Fixture::new("json");
    fs::write(
        &f.source,
        r#"{"scripts":{"caf\u00e9":{"$run":"echo \"hello\"\necho C:\\work\n"}}}"#,
    )
    .unwrap();
    f.add();
    let entry = &f.state()["sources"][0]["entries"][0];
    assert_eq!(entry["key"], json!(["scripts", "café"]));
    assert_eq!(entry["script"]["body"], "echo \"hello\"\necho C:\\work\n");
}

#[test]
fn raw_yaml_and_json_failures_never_replace_saved_state() {
    for (ext, cases) in [
        (
            "yaml",
            vec![
                "web: https://a.test\nweb: https://b.test",
                "x: null",
                "x: yes\ny: false",
                "---\nx: https://a.test\n---\nx: https://b.test",
                "x: [",
                "42: https://a.test",
                "x: !custom https://a.test",
            ],
        ),
        (
            "json",
            vec![
                r#"{"x":"https://a.test","x":"https://b.test"}"#,
                r#"{"x":null}"#,
                r#"{"x":"https://a.test",}"#,
                "{} {}",
                r#"{"x":"bad\q"}"#,
                "[]",
                "null",
            ],
        ),
    ] {
        let f = Fixture::new(ext);
        f.write(json!({"web":"https://a.test"}));
        f.add();
        let before = fs::read(&f.config).unwrap();
        for text in cases {
            fs::write(&f.source, text).unwrap();
            assert!(!f.run(&["reload"]).status.success(), "{ext}: {text}");
            assert!(!f.run(&["web"]).status.success(), "{ext}: {text}");
            assert_eq!(fs::read(&f.config).unwrap(), before);
        }
    }
}

fn alias_metadata(ext: &'static str) {
    let f = Fixture::new(ext);
    f.write(json!({"alias":{"name":"qrl-test-tools"},"web":{"docs":"https://example.com"}}));
    f.add();
    assert_eq!(f.state()["sources"][0]["alias"], "qrl-test-tools");
    assert_eq!(
        f.state()["sources"][0]["entries"].as_array().unwrap().len(),
        1
    );
    for shell in ["bash", "zsh", "fish"] {
        let output = success(f.run(&["__aliases", shell]));
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("qrl-test-tools")
        );
    }
    let reset_attempt = f.run(&["--source", f.source.to_str().unwrap(), "__lookup", "nuke"]);
    assert!(!reset_attempt.status.success());
    assert!(f.config.exists());
    let before = fs::read(&f.config).unwrap();
    for metadata in [
        json!({"name":"qrlkit"}),
        json!({"name":"bad;name"}),
        json!({"name":42}),
        json!({"name":"valid","extra":true}),
        json!("bad"),
    ] {
        f.write(json!({"alias":metadata,"web":"https://example.com"}));
        assert!(!f.run(&["reload"]).status.success());
        assert_eq!(before, fs::read(&f.config).unwrap());
    }
    f.write(json!({"alias":{"name":"qrl-test-renamed"},"web":"https://example.com"}));
    success(f.run(&["reload"]));
    let functions = String::from_utf8(success(f.run(&["__aliases", "bash"])).stdout).unwrap();
    assert!(functions.contains("qrl-test-renamed"));
    assert!(!functions.contains("qrl-test-tools"));
    success(f.run(&["rm", f.source.to_str().unwrap()]));
    assert!(success(f.run(&["__aliases", "bash"])).stdout.is_empty());
}
#[test]
fn toml_alias_metadata() {
    alias_metadata("toml");
}
#[test]
fn yaml_alias_metadata() {
    alias_metadata("yaml");
}
#[test]
fn json_alias_metadata() {
    alias_metadata("json");
}

#[cfg(unix)]
#[test]
fn shell_alias_forwards_arguments_and_changes_directory_with_scoped_names() {
    let f = Fixture::new("toml");
    let project = f.dir.path().join("project ' quoted");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("file.txt"), "").unwrap();
    f.write(
        json!({"alias":{"name":"qrl-test-tools"}, "files":{"one":"./file.txt"},
        "dirs":{"project":project.to_str().unwrap()},
        "tasks":{"run":{"$run":"printf '<%s>\\n' \"$@\"","$shell":"bash"}}}),
    );
    f.add();
    let other = f.dir.path().join("other.json");
    fs::write(&other, r#"{"unrelated":"https://example.com"}"#).unwrap();
    success(f.run(&["add", other.to_str().unwrap()]));
    let mut state = f.state();
    state["sources"][0]["renames"]["files"] = json!("renamed-files");
    for entry in state["sources"][0]["entries"].as_array_mut().unwrap() {
        if entry["key"][0] == "files" {
            entry["key"][0] = json!("renamed-files");
        }
    }
    fs::write(&f.config, serde_yaml_ng::to_string(&state).unwrap()).unwrap();
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_qrlkit")).parent().unwrap();
    let mut paths = vec![binary_dir.to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    for shell in ["bash", "zsh"] {
        if Command::new(shell).arg("--version").output().is_err() {
            continue;
        }
        let init = String::from_utf8(success(f.run(&["init", shell])).stdout).unwrap();
        let aliases = String::from_utf8(success(f.run(&["__aliases", shell])).stdout).unwrap();
        let body = format!(
            "{init}\n{aliases}\nqrl-test-tools dirs project || exit\nqrl-test-tools files one || exit\nqrl-test-tools tasks run 'hello world' --title || exit\nif qrl-test-tools unrelated; then exit 99; fi\n"
        );
        let result = Command::new(shell)
            .args(["-c", &body])
            .env("PATH", std::env::join_paths(&paths).unwrap())
            .current_dir(f.dir.path())
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!(
                "{}\n<hello world>\n<--title>\n",
                fs::canonicalize(project.join("file.txt"))
                    .unwrap()
                    .display()
            )
        );
        let body = format!("qrl-test-tools() {{ echo original; }}\n{aliases}\nqrl-test-tools");
        let result = Command::new(shell).args(["-c", &body]).output().unwrap();
        assert_eq!(result.stdout, b"original\n");
    }
}

#[test]
fn duplicate_alias_import_preserves_state() {
    let f = Fixture::new("yaml");
    f.write(json!({"alias":{"name":"qrl-test-dupe"},"a":"https://a.test"}));
    f.add();
    let other = f.dir.path().join("other.json");
    fs::write(
        &other,
        r#"{"alias":{"name":"qrl-test-dupe"},"b":"https://b.test"}"#,
    )
    .unwrap();
    let before = fs::read(&f.config).unwrap();
    let result = f.run(&["add", other.to_str().unwrap()]);
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("Duplicate alias")
    );
    assert_eq!(before, fs::read(&f.config).unwrap());
}

fn templated_resources(ext: &'static str) {
    let f = Fixture::new(ext);
    let project = f.dir.path().join("space ø");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("prod.txt"), "").unwrap();
    f.write(json!({"alias":{"name":"qrl-test-template"},"file":"./{project}/{env}.txt", "dir":"./{project}","web":"https://example.com/{env}?q={query}&again={query}"}));
    f.add();
    path_output(
        f.run(&["file", "space ø", "prod"]),
        &project.join("prod.txt"),
    );
    path_output(
        f.run(&[
            "--source",
            f.source.to_str().unwrap(),
            "__lookup",
            "file",
            "space ø",
            "prod",
        ]),
        &project.join("prod.txt"),
    );
    let result = f.run(&["file", "space ø"]);
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("interactive terminal")
    );
    let result = f.run(&["file", "space ø", "prod", "extra"]);
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("Expected 2 argument(s), got 3")
    );
    let channel = f.dir.path().join("cd-channel");
    let output = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .current_dir(f.dir.path())
        .env("QRL_CD_FILE", &channel)
        .arg("--config")
        .arg(&f.config)
        .args(["dir", "space ø"])
        .output()
        .unwrap();
    success(output);
    assert_eq!(
        fs::read_to_string(channel).unwrap().trim(),
        fs::canonicalize(&project).unwrap().to_str().unwrap()
    );
    let mut state = f.state();
    state["browser"] = json!({"name":"test","executable":f.dir.path().join("nonexistent-browser").to_str().unwrap(),"args":[]});
    fs::write(&f.config, serde_yaml_ng::to_string(&state).unwrap()).unwrap();
    let result = f.run(&["web", "prod", "a &ø"]);
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("qrlkit: open https://example.com/prod?q=a%20%26%C3%B8&again=a%20%26%C3%B8")
    );
    f.write(json!({"file":"./{env}/{project}.txt"}));
    fs::create_dir(f.dir.path().join("prod")).unwrap();
    fs::write(f.dir.path().join("prod/new.txt"), "").unwrap();
    path_output(
        f.run(&["file", "prod", "new"]),
        &f.dir.path().join("prod/new.txt"),
    );
}
#[test]
fn toml_templates() {
    templated_resources("toml");
}
#[test]
fn yaml_templates() {
    templated_resources("yaml");
}
#[test]
fn json_templates() {
    templated_resources("json");
}

#[test]
fn automatic_root_commands_across_formats_and_lifecycle() {
    for ext in ["toml", "yaml", "json"] {
        let f = Fixture::new(ext);
        f.write(json!({"docs":{"guide":"./guide.txt"},"notes":{"today":"./today.txt"}}));
        f.add();
        let startup = fs::read_to_string(f.dir.path().join(".zshrc")).unwrap();
        assert!(startup.contains("qrlkit --config"));
        // Upgrade previously installed integration without duplicating its loader.
        fs::write(
            f.dir.path().join(".zshrc"),
            startup.replace("qrlkit", "qrl"),
        )
        .unwrap();
        success(f.run(&["reload"]));
        let upgraded = fs::read_to_string(f.dir.path().join(".zshrc")).unwrap();
        assert!(!upgraded.contains("qrl --config"));
        assert!(upgraded.contains("qrlkit()"));
        assert_eq!(upgraded.matches("__aliases zsh").count(), 1);
        for shell in ["bash", "zsh", "fish"] {
            let functions =
                String::from_utf8(success(f.run(&["__aliases", shell])).stdout).unwrap();
            assert!(functions.contains("--root 'docs' __lookup"), "{functions}");
            assert!(functions.contains("--root 'notes' __lookup"), "{functions}");
        }
        f.write(json!({"journal":{"today":"./today.txt"}}));
        success(f.run(&["reload"]));
        let functions = String::from_utf8(success(f.run(&["__aliases", "bash"])).stdout).unwrap();
        assert!(functions.contains("--root 'journal'"));
        assert!(!functions.contains("--root 'docs'"));
        assert!(!functions.contains("--root 'notes'"));
        success(f.run(&["rm", f.source.to_str().unwrap()]));
        assert!(success(f.run(&["__aliases", "bash"])).stdout.is_empty());
        f.add();
        success(f.run(&["nuke"]));
        assert!(success(f.run(&["__aliases", "bash"])).stdout.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn automatic_commands_execute_in_shell_with_scoping_renames_and_conflicts() {
    let f = Fixture::new("toml");
    let project = f.dir.path().join("project ' quoted");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("guide.txt"), "").unwrap();
    f.write(json!({
        "docs":{"guide":"./guide.txt"},
        "notes":{"$run":"printf '<%s>\\n' \"$@\""},
        "qrl-test-jump":project.to_str().unwrap(),
        "echo":"./guide.txt",
        "bad;name":"./guide.txt"
    }));
    f.add();
    let mut state = f.state();
    state["sources"][0]["renames"]["docs"] = json!("team-docs");
    for entry in state["sources"][0]["entries"].as_array_mut().unwrap() {
        if entry["key"][0] == "docs" {
            entry["key"][0] = json!("team-docs");
        }
    }
    fs::write(&f.config, serde_yaml_ng::to_string(&state).unwrap()).unwrap();
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_qrlkit")).parent().unwrap();
    let mut paths = vec![binary_dir.to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    for shell in ["bash", "zsh"] {
        if Command::new(shell).arg("--version").output().is_err() {
            continue;
        }
        let aliases = String::from_utf8(success(f.run(&["__aliases", shell])).stdout).unwrap();
        assert!(!aliases.contains("bad;name"));
        assert!(!aliases.contains("echo()"));
        let body = ". \"$QRL_TEST_STARTUP\"\nqrl-test-jump || exit\nteam-docs guide || exit\nnotes 'hello world' --title || exit\nif team-docs notes; then exit 99; fi\n";
        let result = Command::new(shell)
            .args(["-c", body])
            .env("PATH", std::env::join_paths(&paths).unwrap())
            .env("QRL_TEST_STARTUP", f.dir.path().join(".zshrc"))
            .env("SHELL", "/bin/zsh")
            .env("ZDOTDIR", f.dir.path())
            .current_dir(f.dir.path())
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            format!(
                "{}\n<hello world>\n<--title>\n",
                fs::canonicalize(project.join("guide.txt"))
                    .unwrap()
                    .display()
            )
        );
        let result = Command::new(shell)
            .args([
                "-c",
                &format!("notes() {{ echo original; }}\n{aliases}\nnotes"),
            ])
            .output()
            .unwrap();
        assert_eq!(result.stdout, b"original\n");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("conflicts with an existing command")
        );
    }
}
