use serde_json::json;
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
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("state.yaml"),
        "version: 1\nbrowser: {name: Test, executable: /bin/echo}\nsources: []\n",
    )
    .unwrap();
    dir
}

fn import(root: &Path, text: &str) {
    fs::write(root.join("tools.toml"), text).unwrap();
    success(root, &["add", "tools.toml"]);
}

fn output(root: &Path, args: &[&str]) -> String {
    let result = success(root, args);
    if result.stdout.is_empty()
        && let Some(url) = String::from_utf8_lossy(&result.stderr)
            .lines()
            .find_map(|line| line.strip_prefix("qrlkit: open "))
    {
        return url.to_owned();
    }
    String::from_utf8(result.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

#[test]
fn complete_composition_multiselectors_and_script_results_work_across_formats() {
    for extension in ["toml", "yaml", "json"] {
        let dir = fixture();
        let root = dir.path();
        let config = json!({
            "projects": {"hidden": true, "org": "qrlkit", "repos": {"hint": "Choose a project", "values": [
                {"value": "qrlkit-rs", "hint": "Rust CLI"}, {"value": "qrlkit-web", "hint": "Website"}
            ]}},
            "shared": {"hidden": true, "environments": ["sim", "live"], "regions": ["eu", "us"]},
            "system": {"hidden": true, "hostname": {"run": "printf 'host\\n'; printf x >> calls"}},
            "logs": {"hint": "Search logs", "url": "https://{logs.hosts[projects.repos, shared.environments, shared.regions]}.example.com/?service={projects.repos}&host={system.hostname}&again={system.hostname}",
                "hosts": {"hidden": true,
                    "qrlkit-rs": {"sim": {"eu": "rust-sim-eu", "us": "rust-sim-us"}, "live": {"eu": "rust-live-eu", "us": "rust-live-us"}},
                    "qrlkit-web": {"sim": {"eu": "web-sim-eu", "us": "web-sim-us"}, "live": {"eu": "web-live-eu", "us": "web-live-us"}}
                }},
            "repos": {"hint": "GitHub resources", "open": {"url": "https://github.com/{projects.org}/{projects.repos}"},
                "prs": {"hint": "Pull requests", "url": "{repos.open}/pulls"}}
        });
        let text = match extension {
            "toml" => toml::to_string(&config).unwrap(),
            "yaml" => serde_yaml_ng::to_string(&config).unwrap(),
            _ => config.to_string(),
        };
        let name = format!("tools.{extension}");
        fs::write(root.join(&name), text).unwrap();
        success(root, &["add", &name]);
        success(root, &["reload"]);
        success(root, &["ls"]);
        assert!(!root.join("calls").exists());
        assert_eq!(
            output(root, &["logs", "qrlkit-rs", "live", "eu"]),
            "https://rust-live-eu.example.com/?service=qrlkit-rs&host=host&again=host"
        );
        assert_eq!(fs::read_to_string(root.join("calls")).unwrap(), "x");
        assert_eq!(
            output(root, &["logs", "qrlkit-web", "sim", "us"]),
            "https://web-sim-us.example.com/?service=qrlkit-web&host=host&again=host"
        );
        assert_eq!(fs::read_to_string(root.join("calls")).unwrap(), "xx");
        assert_eq!(
            output(root, &["repos", "prs", "qrlkit-web"]),
            "https://github.com/qrlkit/qrlkit-web/pulls"
        );
    }
}

#[test]
fn shared_values_are_resolved_after_all_files_in_a_directory_are_loaded() {
    let dir = fixture();
    let root = dir.path();
    let configs = root.join("configs");
    fs::create_dir(&configs).unwrap();
    fs::write(
        configs.join("a.toml"),
        "[view]\nurl='https://example.com/{shared.names}'\n",
    )
    .unwrap();
    fs::write(
        configs.join("z.yaml"),
        "shared:\n  hidden: true\n  names: [first, second]\n",
    )
    .unwrap();
    success(root, &["add", "configs"]);
    assert_eq!(
        output(root, &["view", "second"]),
        "https://example.com/second"
    );
    fs::write(
        configs.join("z.yaml"),
        "shared:\n  hidden: true\n  names: [third]\n",
    )
    .unwrap();
    assert!(!run(root, &["view", "second"]).status.success());
    assert_eq!(
        output(root, &["view", "third"]),
        "https://example.com/third"
    );
}

#[test]
fn plain_values_and_lists_can_be_called_even_when_hidden() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        "[projects]\nhidden=true\norg='qrlkit'\nrepos=['rs','web']\n",
    );
    assert_eq!(output(root, &["projects", "org"]), "qrlkit");
    assert_eq!(output(root, &["projects", "repos", "web"]), "web");
    assert!(
        !run(root, &["projects", "repos", "missing"])
            .status
            .success()
    );
    let aliases = output(root, &["__aliases", "zsh"]);
    assert!(aliases.contains("projects()"), "{aliases}");
    let state: serde_json::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join("state.yaml")).unwrap()).unwrap();
    assert_eq!(state["version"], 2);
    assert!(
        state["sources"][0]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| group["hidden"] == true)
    );
}

#[test]
fn action_groups_prefer_explicit_children_and_double_dash_disambiguates_choices() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        "[data]\nvalues=['child','other']\n[tool]\nurl='https://example.com/{data}'\n[tool.child]\nurl='https://child.example.com'\n",
    );
    assert_eq!(
        output(root, &["tool", "other"]),
        "https://example.com/other"
    );
    assert_eq!(
        output(root, &["tool", "child"]),
        "https://child.example.com"
    );
    assert_eq!(
        output(root, &["tool", "--", "child"]),
        "https://example.com/child"
    );
}

#[test]
fn composed_urls_encode_dynamic_values_once_and_preserve_url_separators() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[shared]
hidden=true
org='qrlkit'
names=['a /?&ø', 'a%2Fb']
[repos.open]
url='https://github.com/{shared.org}/{shared.names}'
[repos.prs]
url='{repos.open}/pulls?again={shared.names}'
"#,
    );
    assert_eq!(
        output(root, &["repos", "prs", "a /?&ø"]),
        "https://github.com/qrlkit/a%20%2F%3F%26%C3%B8/pulls?again=a%20%2F%3F%26%C3%B8"
    );
    assert_eq!(
        output(root, &["repos", "prs", "a%2Fb"]),
        "https://github.com/qrlkit/a%252Fb/pulls?again=a%252Fb"
    );
}

#[test]
fn selected_values_and_script_output_are_literal_not_new_expressions() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[data]
hidden=true
choices=['{missing.reference}; $(touch injected)']
[dynamic]
hidden=true
run="printf '%s' '{also.missing}; $(touch injected)'"
[show]
run='printf "%s\n%s\n" "$VALUE" "$DYNAMIC"'
args=[{name='value',enum={ref='data.choices'}}]
[show.env]
VALUE='{data.choices}'
DYNAMIC='{dynamic}'
"#,
    );
    assert_eq!(
        output(root, &["show", "{missing.reference}; $(touch injected)"]),
        "{missing.reference}; $(touch injected)\n{also.missing}; $(touch injected)"
    );
    assert!(!root.join("injected").exists());
}

#[test]
fn invalid_arguments_and_missing_lookup_combinations_do_not_run_dependencies() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[data]
hidden=true
repos=['rs','web']
environments=['sim','live']
[dynamic]
hidden=true
run='touch captured; printf host'
[logs]
url='https://{dynamic}.example.com/{logs.names[data.repos,data.environments]}'
[logs.names]
hidden=true
[logs.names.rs]
sim='sandbox'
[logs.names.web]
live='production'
"#,
    );
    for args in [
        vec!["logs", "unknown", "sim"],
        vec!["logs", "rs", "unknown"],
        vec!["logs", "rs", "sim", "extra"],
        vec!["logs", "rs", "live"],
    ] {
        assert!(!run(root, &args).status.success(), "{args:?}");
        assert!(!root.join("captured").exists(), "{args:?}");
    }
    assert_eq!(
        output(root, &["logs", "web", "live"]),
        "https://host.example.com/production"
    );
}

#[test]
fn nested_lookup_expressions_select_keys_without_splitting_selected_values() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[data]
hidden=true
repos=['rust.project','web/project']
regions=['eu']
[data.keys]
'rust.project'='rs'
'web/project'='web'
[logs]
url='https://{logs.hosts[data.keys[data.repos],data.regions]}.example.com'
[logs.hosts]
hidden=true
[logs.hosts.rs]
eu='rust'
[logs.hosts.web]
eu='web'
"#,
    );
    assert_eq!(
        output(root, &["logs", "rust.project", "eu"]),
        "https://rust.example.com"
    );
    assert_eq!(
        output(root, &["logs", "web/project", "eu"]),
        "https://web.example.com"
    );
}

#[test]
fn dependencies_of_unselected_branches_are_not_executed_or_prompted() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[data]
hidden=true
repos=['rs','web']
unused=['first','second']
[dynamic]
hidden=true
run='touch ran; printf web'
[logs]
url='https://example.com/{logs.paths[data.repos]}'
[logs.paths]
hidden=true
rs='rust'
web='{dynamic}/{data.unused}'
"#,
    );
    assert_eq!(output(root, &["logs", "rs"]), "https://example.com/rust");
    assert!(!root.join("ran").exists());
    assert_eq!(
        output(root, &["logs", "web", "second"]),
        "https://example.com/web/second"
    );
}

#[test]
fn scripts_capture_from_their_source_directory_with_literal_environment_and_stderr() {
    let dir = fixture();
    let root = dir.path();
    let configs = root.join("configs");
    fs::create_dir(&configs).unwrap();
    fs::write(
        configs.join("source.toml"),
        r#"
[data]
hidden=true
value='$(touch injected); spaces'
[dynamic]
hidden=true
run='printf diagnostic >&2; printf "%s\n%s\n" "$PWD" "$LITERAL"'
[dynamic.env]
LITERAL='{data.value}'
[show]
run='printf "%s" "$RESULT"'
[show.env]
RESULT='{dynamic}'
"#,
    )
    .unwrap();
    success(root, &["add", "configs/source.toml"]);
    let result = success(root, &["show"]);
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!(
            "{}\n$(touch injected); spaces",
            configs.canonicalize().unwrap().display()
        )
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("diagnostic"));
    assert!(!root.join("injected").exists());
    assert!(!configs.join("injected").exists());
}

#[test]
fn referenced_script_failure_and_invalid_output_stop_the_outer_action() {
    for script in ["printf nope; exit 7", "printf '\\377'", "printf '\\0'"] {
        let dir = fixture();
        let root = dir.path();
        import(
            root,
            &format!(
                "[dynamic]\nhidden=true\nrun=\"\"\"{}\"\"\"\n[outer]\nrun='touch outer-ran'\n[outer.env]\nVALUE='{{dynamic}}'\n",
                script.replace('\\', "\\\\")
            ),
        );
        let result = run(root, &["outer"]);
        assert!(!result.status.success(), "{script}: {result:?}");
        assert!(!root.join("outer-ran").exists());
    }
}

#[test]
fn cycles_and_missing_references_fail_reload_atomically_without_running_scripts() {
    let dir = fixture();
    let root = dir.path();
    import(root, "[task]\nrun='true'\n");
    let before = fs::read(root.join("state.yaml")).unwrap();
    for source in [
        "[a]\nurl='{a}'",
        "[a]\nurl='{b}'\n[b]\nurl='{a}'",
        "[a]\nrun='touch ran'\n[a.env]\nX='{b}'\n[b]\nrun='touch ran'\n[b.env]\nX='{a}'",
        "[a]\nurl='https://example.com/{missing.value}'",
        "[data]\nvalues=['a']\n[a]\nurl='https://example.com/{a.paths[data]}'\n[a.paths]\na='{a}'",
    ] {
        fs::write(root.join("tools.toml"), source).unwrap();
        assert!(!run(root, &["reload"]).status.success(), "{source}");
        assert_eq!(before, fs::read(root.join("state.yaml")).unwrap());
        assert!(!root.join("ran").exists());
    }
}

#[test]
fn malformed_values_metadata_and_expressions_fail_atomically() {
    let dir = fixture();
    let root = dir.path();
    import(root, "[task]\nrun='true'\n");
    let before = fs::read(root.join("state.yaml")).unwrap();
    for source in [
        "[data]\nhidden='true'\nx='a'",
        "[data]\nx=true",
        "[data]\nvalues=[]",
        "[data]\nvalues=['a','a']",
        "[data]\nvalues=[{value='a',typo='b'}]",
        "[data]\nvalues=[{hint='a'}]",
        "[data]\nvalues=[1]",
        "[data]\nvalues=['a']\nchild='b'",
        "[data]\nurl='./file'\nrun='true'",
        "[data]\nurl='./file'\nvalues=['a']",
        "[data]\nurl='https://example.com/{missing[]}'",
        "[data]\nurl='https://example.com/{missing[a,]}'",
        "[data]\nurl='https://example.com/{missing[a,,b]}'",
        "[data]\nurl='https://example.com/{missing[a][b]}'",
        "[data]\nurl='https://example.com/{missing[a}'",
        "[data]\nurl='https://example.com/{.missing}'",
        "[data]\nurl='https://example.com/{missing..x}'",
        "[data]\nurl='https://example.com/{missing()}'",
    ] {
        fs::write(root.join("tools.toml"), source).unwrap();
        assert!(!run(root, &["reload"]).status.success(), "{source}");
        assert_eq!(before, fs::read(root.join("state.yaml")).unwrap());
    }
}

#[test]
fn constants_file_is_no_longer_automatically_loaded() {
    let dir = fixture();
    let root = dir.path();
    fs::write(root.join("constants.toml"), "broken = [").unwrap();
    import(root, "[task]\nrun='printf done'\n");
    assert_eq!(output(root, &["task"]), "done");
}

#[test]
fn resource_arguments_and_hints_work_across_formats() {
    for extension in ["toml", "yaml", "json"] {
        let dir = fixture();
        let root = dir.path();
        fs::create_dir(root.join("repo with spaces")).unwrap();
        fs::write(root.join("repo with spaces/README.md"), "hello").unwrap();
        let config = json!({
            "shared": {"hidden": true, "repos": ["repo with spaces"], "files": ["README.md"]},
            "files": {"read": {"hint": "Read a file", "url": "./{repo}/{file}", "args": [
                {"name": "file", "enum": {"ref": "shared.files"}}, {"name": "repo", "enum": {"ref": "shared.repos"}}
            ]}}
        });
        let text = match extension {
            "toml" => toml::to_string(&config).unwrap(),
            "yaml" => serde_yaml_ng::to_string(&config).unwrap(),
            _ => config.to_string(),
        };
        let name = format!("tools.{extension}");
        fs::write(root.join(&name), text).unwrap();
        success(root, &["add", &name]);
        assert_eq!(
            output(root, &["files", "read", "README.md", "repo with spaces"]),
            root.join("repo with spaces/README.md")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
        assert!(
            !run(root, &["files", "read", "missing", "repo with spaces"])
                .status
                .success()
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

#[test]
fn invalid_resource_arguments_leave_state_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("resources.toml"), "[files]\nfile='./{name}'\n").unwrap();
    success(root, &["add", "resources.toml"]);
    let before = fs::read(root.join("state.yaml")).unwrap();
    for args in [
        "[]",
        "[{name='other'}]",
        "[{name='name'}, {name='name'}]",
        "[{name='name',enum=[]}]",
        "[{name='name',enum=['a','a']}]",
        "[{name='name',enum={ref='shared.missing'}}]",
        "[{name='name',enum='a'}]",
    ] {
        fs::write(
            root.join("resources.toml"),
            format!("[files.file]\nurl='./{{name}}'\nargs={args}\n"),
        )
        .unwrap();
        assert!(!run(root, &["reload"]).status.success(), "{args}");
        assert_eq!(fs::read(root.join("state.yaml")).unwrap(), before);
    }
}

#[test]
fn scripts_can_produce_a_complete_target_without_encoding_its_url_separators() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        "[dynamic]\nhidden=true\nrun=\"printf 'https://example.com/base?x=1'\"\n[open]\nurl='{dynamic}&y=2'\n",
    );
    assert_eq!(output(root, &["open"]), "https://example.com/base?x=1&y=2");
}

#[test]
fn captured_output_preserves_spaces_and_interior_newlines() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[dynamic]
hidden=true
run="printf ' x \n y \r\n\r\n'"
[show]
run='printf "<%s>" "$VALUE"'
[show.env]
VALUE='{dynamic}'
"#,
    );
    assert_eq!(success(root, &["show"]).stdout, b"< x \n y >");
}

#[test]
fn source_aliases_can_resolve_dependencies_in_other_registered_configs() {
    let dir = fixture();
    let root = dir.path();
    fs::write(
        root.join("shared.yaml"),
        "shared:\n  hidden: true\n  names: [rs, web]\n",
    )
    .unwrap();
    success(root, &["add", "shared.yaml"]);
    import(
        root,
        "[alias]\nname='project-tools'\n[repos.open]\nurl='https://example.com/{shared.names}'\n[repos.prs]\nurl='{repos.open}/pulls'\n",
    );
    // Simulate a saved namespace rename, as the existing collision tests do.
    let mut state: serde_json::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join("state.yaml")).unwrap()).unwrap();
    let source = &mut state["sources"][1];
    source["renames"]["repos"] = json!("renamed-repos");
    for entry in source["entries"].as_array_mut().unwrap() {
        entry["key"][0] = json!("renamed-repos");
    }
    fs::write(
        root.join("state.yaml"),
        serde_yaml_ng::to_string(&state).unwrap(),
    )
    .unwrap();
    assert_eq!(
        output(root, &["renamed-repos", "prs", "web"]),
        "https://example.com/web/pulls"
    );
    assert_eq!(
        output(
            root,
            &["--source", "tools.toml", "__lookup", "repos", "prs", "rs"]
        ),
        "https://example.com/rs/pulls"
    );
}

#[test]
fn the_complete_shipped_composition_example_imports_and_composes() {
    let dir = fixture();
    let root = dir.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/composition.toml");
    success(root, &["add", source.to_str().unwrap()]);
    assert_eq!(
        output(root, &["repos", "prs", "qrlkit-web"]),
        "https://github.com/qrlkit/qrlkit-web/pulls"
    );
    let value = output(root, &["logs", "qrlkit-rs", "worker", "live", "eu"]);
    assert!(
        value.starts_with(
            "https://rust-worker.production.eu.logs.example.com/search?service=qrlkit-rs&host="
        ),
        "{value}"
    );
}

#[test]
fn an_action_can_share_its_namespace_with_children_from_another_config() {
    let dir = fixture();
    let root = dir.path();
    let configs = root.join("configs");
    fs::create_dir(&configs).unwrap();
    fs::write(configs.join("action.toml"), "[logs]\nurl='https://{logs.hosts[data.repos]}.example.com'\n[logs.child]\nurl='https://child.example.com'\n").unwrap();
    fs::write(configs.join("data.toml"), "[data]\nhidden=true\nrepos=['rs','web']\n[logs.hosts]\nhidden=true\nrs='rust'\nweb='web'\n").unwrap();
    success(root, &["add", "configs"]);
    assert_eq!(output(root, &["logs", "rs"]), "https://rust.example.com");
    assert_eq!(
        output(root, &["logs", "child"]),
        "https://child.example.com"
    );
    assert_eq!(output(root, &["logs", "hosts", "web"]), "web");
}

#[test]
fn nested_scalar_composition_preserves_literal_separators_and_encodes_choices_once() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[shared]
hidden=true
names=['a /?&ø']
[data]
hidden=true
prefix='team/{shared.names}'
wrapped='{data.prefix}'
[page]
url='https://example.com/{data.wrapped}/end?again={shared.names}'
"#,
    );
    assert_eq!(
        output(root, &["page", "a /?&ø"]),
        "https://example.com/team/a%20%2F%3F%26%C3%B8/end?again=a%20%2F%3F%26%C3%B8"
    );
}

#[test]
fn a_composed_url_can_itself_supply_an_encoded_lookup_key() {
    let dir = fixture();
    let root = dir.path();
    import(
        root,
        r#"
[data]
hidden=true
names=['a b']
[base]
hidden=true
url='https://example.com/{data.names}'
[routes]
hidden=true
'https://example.com/a%20b'='matched'
[page]
url='https://example.com/{routes[base]}'
"#,
    );
    assert_eq!(
        output(root, &["page", "a b"]),
        "https://example.com/matched"
    );
}
