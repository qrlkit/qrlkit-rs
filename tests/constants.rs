use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_qrlkit"))
        .current_dir(root)
        .env("HOME", root)
        .env("ZDOTDIR", root)
        .env("SHELL", "/bin/zsh")
        .args(["--config", root.join("state.yaml").to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}

fn success(root: &Path, args: &[&str]) -> Output {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn enums_and_environment_are_literal_and_reloaded_before_execution() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("constants.toml"),
        "[constants]\nroot = '~/dev'\nregions = ['eu', 'us']\n",
    )
    .unwrap();
    fs::write(
        root.join("tasks.toml"),
        r#"
[constants]
environments = ['dev', 'staging', 'prod']
literal = '$(touch injected); spaces'
[deploy]
run = '''printf '%s\n' "$1" "$2" "$PROJECT_ROOT" "$LITERAL"; touch ran'''
args = [
  { name = 'environment', enum = { ref = 'constants.environments' } },
  { name = 'region', enum = { ref = 'global.regions' } },
]
[deploy.env]
PROJECT_ROOT = '{global.root}/api'
LITERAL = '{constants.literal}'
"#,
    )
    .unwrap();
    success(root, &["add", "tasks.toml"]);
    for args in [
        vec!["deploy", "stagign", "eu"],
        vec!["deploy", "dev", "eu", "extra"],
        vec!["deploy", "dev"],
    ] {
        let output = run(root, &args);
        assert!(!output.status.success());
        assert!(!root.join("ran").exists());
    }
    let output = success(root, &["deploy", "--", "staging", "eu"]);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "staging\neu\n{}/dev/api\n$(touch injected); spaces\n",
            root.display()
        )
    );
    assert!(!root.join("injected").exists());
    fs::remove_file(root.join("ran")).unwrap();
    fs::write(
        root.join("constants.toml"),
        "[constants]\nroot = '~/work'\nregions = ['apac']\n",
    )
    .unwrap();
    assert!(!run(root, &["deploy", "dev", "eu"]).status.success());
    assert!(!root.join("ran").exists());
    let output = success(root, &["deploy", "prod", "apac"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("/work/api"));
}

#[test]
fn constants_are_scoped_and_combine_with_user_placeholders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("constants.toml"),
        "[constants]\nroot = './global'\n",
    )
    .unwrap();
    fs::create_dir(root.join("local")).unwrap();
    fs::create_dir(root.join("global")).unwrap();
    fs::write(root.join("local/a"), "a").unwrap();
    fs::write(root.join("global/b"), "b").unwrap();
    fs::write(root.join("one.toml"), "[constants]\nroot = './local'\n[files]\nlocal = '{constants.root}/{name}'\nshared = '{global.root}/{name}'\n").unwrap();
    success(root, &["add", "one.toml"]);
    assert!(
        String::from_utf8_lossy(&success(root, &["files", "local", "a"]).stdout)
            .contains("/local/a")
    );
    assert!(
        String::from_utf8_lossy(&success(root, &["files", "shared", "b"]).stdout)
            .contains("/global/b")
    );
    fs::write(
        root.join("two.toml"),
        "[other]\nfile = '{constants.root}/a'\n",
    )
    .unwrap();
    let before = fs::read(root.join("state.yaml")).unwrap();
    let output = run(root, &["add", "two.toml"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown constant"));
    assert_eq!(before, fs::read(root.join("state.yaml")).unwrap());
}

#[test]
fn invalid_declarations_fail_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("task.toml"), "[task]\nrun = 'touch ran'\n").unwrap();
    success(root, &["add", "task.toml"]);
    let before = fs::read(root.join("state.yaml")).unwrap();
    for source in [
        "[constants]\nx = 1\n[task]\nrun = 'true'",
        "[constants]\nx = '{global.x}'\n[task]\nrun = 'true'",
        "[constants]\nx = ['a']\n[files]\na = '{constants.x}'",
        "[files]\na = '{global.missing}'",
        "[task]\nrun = 'true'\nargs = [{name='x', enum=[]}]",
        "[task]\nrun = 'true'\nargs = [{name='x', enum=['a','a']}]",
        "[task]\nrun = 'true'\nargs = [{name='x', enum=['a']}, {name='x', enum=['b']}]",
        "[task]\nrun = 'true'\nargs = [{name='x', enum={ref='constants.missing'}}]",
        "[constants]\nx = 'a'\n[task]\nrun = 'true'\nargs = [{name='x', enum={ref='constants.x'}}]",
        "[task]\nrun = 'true'\nargs = [{name='x', enum=['a'], typo='b'}]",
        "[task]\nrun = 'true'\nenv = {'bad-name'='a'}",
        "[task]\nrun = 'true'\nenv = {QRL_CD_FILE='a'}",
        "[task]\nrun = 'true'\nenv = {FOO='{global.missing}'}",
        "[files]\na = []",
    ] {
        fs::write(root.join("task.toml"), source).unwrap();
        let output = run(root, &["reload"]);
        assert!(!output.status.success(), "{source}");
        assert_eq!(
            before,
            fs::read(root.join("state.yaml")).unwrap(),
            "{source}"
        );
    }
    assert!(!root.join("ran").exists());
}

#[test]
fn structured_arguments_and_hints_work_across_formats() {
    for (extension, source) in [
        (
            "toml",
            "[constants]\nvalues=['first','second']\n[task]\nrun='printf %s \"$1\"'\nargs=[{name='value',enum={ref='constants.values'}}]\n[files]\n# hint: Still a resource hint\nfile='./file'",
        ),
        (
            "yaml",
            "constants:\n  values: [first, second]\ntask:\n  run: 'printf %s \"$1\"'\n  args:\n    - name: value\n      enum: {ref: constants.values}\nfiles:\n  # hint: Still a resource hint\n  file: ./file\n",
        ),
        (
            "json",
            r#"{"constants":{"values":["first","second"]},"task":{"run":"printf %s \"$1\"","args":[{"name":"value","enum":{"ref":"constants.values"}}]},"files":{"file":{"url":"./file","hint":"Still a resource hint"}}}"#,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = format!("tasks.{extension}");
        fs::write(root.join(&path), source).unwrap();
        success(root, &["add", &path]);
        assert_eq!(success(root, &["task", "second"]).stdout, b"second");
        assert!(!run(root, &["task", "third"]).status.success());
        assert!(
            fs::read_to_string(root.join("state.yaml"))
                .unwrap()
                .contains("Still a resource hint")
        );
    }
}

#[test]
fn declared_empty_arguments_reject_inputs_while_legacy_scripts_accept_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("task.toml"),
        "[strict]\nrun='printf strict'\nargs=[]\n[legacy]\nrun='printf %s \"$1\"'\n",
    )
    .unwrap();
    success(root, &["add", "task.toml"]);
    assert_eq!(success(root, &["strict"]).stdout, b"strict");
    assert!(!run(root, &["strict", "extra"]).status.success());
    assert_eq!(success(root, &["legacy", "anything"]).stdout, b"anything");
}

#[test]
fn metadata_stays_literal_and_nested_metadata_names_remain_resources() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("tasks.toml"),
        r#"
[constants]
root = './files'
[files]
alias = '{constants.root}/a'
hint = '{constants.root}/b'
[files.described]
url = '{constants.root}/c'
hint = 'Literal {global.missing}'
[task]
run = 'printf %s "{global.missing}"'
"#,
    )
    .unwrap();
    success(root, &["add", "tasks.toml"]);
    let state = fs::read_to_string(root.join("state.yaml")).unwrap();
    for value in [
        "./files/a",
        "./files/b",
        "./files/c",
        "Literal {global.missing}",
    ] {
        assert!(state.contains(value), "{value}");
    }
    assert_eq!(success(root, &["task"]).stdout, b"{global.missing}");
}

#[test]
fn broken_globals_fail_reload_atomically_and_do_not_prevent_removal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("task.toml"), "[task]\nrun='touch ran'\n").unwrap();
    success(root, &["add", "task.toml"]);
    let before = fs::read(root.join("state.yaml")).unwrap();
    for source in ["broken = [", "[constants]\nx = 1", "[other]\nx = 'a'"] {
        fs::write(root.join("constants.toml"), source).unwrap();
        assert!(!run(root, &["task"]).status.success());
        assert_eq!(before, fs::read(root.join("state.yaml")).unwrap());
        assert!(!root.join("ran").exists());
    }
    success(root, &["rm", "task.toml"]);
}

#[test]
fn project_release_arguments_validate_without_running_release_script() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Keep the real project's declarations, substituting a harmless script body.
    let mut config: toml::Value = toml::from_str(include_str!("../QRLs.toml")).unwrap();
    config["qk"]["prep-release"]["run"] =
        toml::Value::String("printf '%s\\n' \"$1\" \"$2\"; touch ran".into());
    fs::write(root.join("tasks.toml"), toml::to_string(&config).unwrap()).unwrap();
    success(root, &["add", "tasks.toml"]);
    let output = run(root, &["qk", "prep-release", "ptach", "Bump deps"]);
    assert!(!output.status.success());
    assert!(!root.join("ran").exists());
    assert!(!run(root, &["qk", "prep-release", "patch"]).status.success());
    assert!(!root.join("ran").exists());
    let note = "Bump deps; $(touch injected)";
    let output = success(root, &["qk", "prep-release", "patch", note]);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("patch\n{note}\n")
    );
    assert!(!root.join("injected").exists());
}

#[test]
fn shipped_examples_execute_mixed_arguments_and_environment_across_formats() {
    for extension in ["toml", "yaml", "json"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(format!("syntax.{extension}"));
        success(root, &["add", source.to_str().unwrap()]);
        let output = success(root, &["scripts", "prepare", "patch", "Bump deps", "eu"]);
        assert_eq!(
            output.stdout,
            b"Release: patch\nNote: Bump deps\nRegion: eu\nProject: /home/bob/work/project\n",
            "{extension}"
        );
        for args in [
            ["scripts", "prepare", "ptach", "Bump deps", "eu"],
            ["scripts", "prepare", "patch", "Bump deps", "unknown"],
        ] {
            let output = run(root, &args);
            assert!(!output.status.success(), "{extension}");
            assert!(output.stdout.is_empty(), "{extension}");
        }
        let output = success(root, &["scripts", "greet", "friend"]);
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("Hello, friend!\n"));
    }
}
