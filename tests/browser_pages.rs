#![cfg(unix)]

use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn browser_pages_import_reload_and_launch_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.yaml");
    let source = dir.path().join("browser.toml");
    let capture = dir.path().join("opened.txt");
    let script = dir.path().join("browser.sh");
    fs::write(&script, "printf '%s' \"$2\" > \"$1\"\n").unwrap();
    fs::write(&state, format!(
        "version: 1\nbrowser:\n  name: Test\n  executable: /bin/sh\n  args: [{:?}, {:?}]\nsources: []\n",
        script.to_str().unwrap(), capture.to_str().unwrap()
    )).unwrap();
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
            .current_dir(dir.path())
            .env("SHELL", "/bin/zsh")
            .env("ZDOTDIR", dir.path())
            .arg("--config")
            .arg(&state)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    for (index, url) in [
        "chrome://extensions/",
        "chrome://settings/",
        "edge://extensions/",
        "brave://settings/",
        "vivaldi://extensions/",
        "opera://settings/",
        "about:preferences#privacy",
        "about:addons",
    ]
    .iter()
    .enumerate()
    {
        fs::write(&source, format!("[bl]\npage = {url:?}\n")).unwrap();
        if index == 0 {
            run(&["add", source.to_str().unwrap()]);
        } else {
            run(&["reload"]);
        }
        fs::write(&capture, "").unwrap();
        run(&["bl", "page"]);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if fs::read_to_string(&capture).unwrap() == *url {
                break;
            }
            assert!(Instant::now() < deadline, "Browser did not receive {url}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn browser_overrides_persist_reload_and_leave_default_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.yaml");
    let source = dir.path().join("resources.toml");
    let capture = dir.path().join("opened.txt");
    let make_browser = |name: &str| {
        let path = dir.path().join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s:%s' '{name}' \"$1\" > '{}'\n",
                capture.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let global = make_browser("global");
    let local = make_browser("local browser");
    let group = make_browser("group");
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_qrlkit"))
            .current_dir(dir.path())
            .env("SHELL", "/bin/zsh")
            .env("ZDOTDIR", dir.path())
            .arg("--config")
            .arg(&state)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let check = |args: &[&str], expected: &str| {
        fs::write(&capture, "").unwrap();
        run(args);
        let deadline = Instant::now() + Duration::from_secs(5);
        while fs::read_to_string(&capture).unwrap() != expected {
            assert!(Instant::now() < deadline, "Expected {expected}");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    fs::write(&source, format!(
        "browser = {:?}\na = 'https://example.com/'\n[work]\nbrowser = {:?}\nb = 'about:addons'\n[work.default]\nbrowser = ''\nc = 'https://example.com/'\n",
        local.to_str().unwrap(), group.to_str().unwrap()
    )).unwrap();
    run(&["add", source.to_str().unwrap()]);
    // An override works before a global browser has been chosen.
    check(&["a"], "local browser:https://example.com/");
    let mut saved: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&state).unwrap()).unwrap();
    assert!(saved["browser"].is_null());
    saved["browser"] = serde_yaml_ng::to_value(serde_json::json!({
        "name": "global", "executable": global, "args": []
    }))
    .unwrap();
    fs::write(&state, serde_yaml_ng::to_string(&saved).unwrap()).unwrap();
    let before = fs::read_to_string(&state).unwrap();
    check(&["work", "b"], "group:about:addons");
    check(&["work", "default", "c"], "global:https://example.com/");
    assert_eq!(fs::read_to_string(&state).unwrap(), before);
    fs::write(&source, "a = 'https://example.com/'\n").unwrap();
    run(&["reload"]);
    check(&["a"], "global:https://example.com/");
}
