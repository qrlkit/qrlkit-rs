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
