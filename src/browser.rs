use crate::{store::Browser, ui};
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

pub fn discover() -> Vec<Browser> {
    let mut found = Vec::new();
    let mut add = |name: &str, path: PathBuf| {
        if path.is_file() && !found.iter().any(|b: &Browser| b.executable == path) {
            found.push(Browser {
                name: name.into(),
                executable: path,
                args: vec![],
            });
        }
    };
    if cfg!(target_os = "macos") {
        let mut roots = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
        ];
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join("Applications"));
        }
        for root in roots {
            for (name, executable) in [
                ("Safari", "Safari.app/Contents/MacOS/Safari"),
                (
                    "Google Chrome",
                    "Google Chrome.app/Contents/MacOS/Google Chrome",
                ),
                ("Firefox", "Firefox.app/Contents/MacOS/firefox"),
                (
                    "Microsoft Edge",
                    "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                ),
                ("Brave", "Brave Browser.app/Contents/MacOS/Brave Browser"),
                ("Arc", "Arc.app/Contents/MacOS/Arc"),
                ("Vivaldi", "Vivaldi.app/Contents/MacOS/Vivaldi"),
            ] {
                add(name, root.join(executable));
            }
        }
    } else if cfg!(target_os = "windows") {
        for root in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
        {
            for (name, executable) in [
                ("Google Chrome", "Google/Chrome/Application/chrome.exe"),
                ("Microsoft Edge", "Microsoft/Edge/Application/msedge.exe"),
                ("Firefox", "Mozilla Firefox/firefox.exe"),
                ("Brave", "BraveSoftware/Brave-Browser/Application/brave.exe"),
                ("Vivaldi", "Vivaldi/Application/vivaldi.exe"),
                ("Opera", "Programs/Opera/opera.exe"),
            ] {
                add(name, PathBuf::from(&root).join(executable));
            }
        }
    } else {
        let mut paths: Vec<_> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        paths.extend(["/usr/bin", "/usr/local/bin", "/snap/bin"].map(PathBuf::from));
        for root in paths {
            for name in [
                "firefox",
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "brave-browser",
                "microsoft-edge",
                "vivaldi",
                "opera",
            ] {
                add(name, root.join(name));
            }
        }
    }
    found
}

pub fn choose() -> Result<Browser> {
    let browsers = discover();
    let mut labels: Vec<_> = browsers
        .iter()
        .map(|b| format!("{} ({})", b.name, b.executable.display()))
        .collect();
    labels.push("Enter browser executable path…".into());
    let index = ui::select("Choose the browser QRL will use", &labels)?;
    if let Some(browser) = browsers.get(index) {
        return Ok(browser.clone());
    }
    let path = PathBuf::from(ui::input("Browser executable path (without quotes)")?);
    anyhow::ensure!(
        path.is_file(),
        "Browser executable not found: {}",
        path.display()
    );
    let path = std::fs::canonicalize(path)?;
    Ok(Browser {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        executable: path,
        args: vec![],
    })
}

pub fn open(browser: &Browser, url: &str) -> Result<()> {
    crate::import::validate_url(url)?;
    // macOS uses Launch Services to address an already running app correctly.
    let mut command = if cfg!(target_os = "macos")
        && browser
            .executable
            .ancestors()
            .any(|p| p.extension().is_some_and(|e| e == "app"))
    {
        let app = browser
            .executable
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .unwrap();
        let mut command = Command::new("/usr/bin/open");
        command.arg("-a").arg(app).arg(url);
        if !browser.args.is_empty() {
            command.arg("--args").args(&browser.args);
        }
        command
    } else {
        let mut command = Command::new(&browser.executable);
        command.args(&browser.args).arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Cannot launch {}; run qrlkit set-browser", browser.name))?;
    Ok(())
}
