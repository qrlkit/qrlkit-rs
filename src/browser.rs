use crate::{store::Browser, ui};
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
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
    } else {
        let mut paths: Vec<_> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        paths.extend(["/usr/bin", "/usr/local/bin", "/snap/bin"].map(PathBuf::from));
        for root in &paths {
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
        if is_wsl() {
            let roots = windows_install_roots(Path::new("powershell.exe"), Path::new("wslpath"));
            for browser in discover_windows(&paths, &roots) {
                add(&browser.name, browser.executable);
            }
        }
    }
    found
}

fn is_wsl() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var_os("WSL_DISTRO_NAME").is_some()
            || std::env::var_os("WSL_INTEROP").is_some()
            || std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft")))
}

fn windows_install_roots(powershell: &Path, wslpath: &Path) -> Vec<PathBuf> {
    // Query Windows itself: the Windows user and drive mounts can differ from Linux.
    let output = Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); \
             [Environment]::GetFolderPath('ProgramFiles'); \
             [Environment]::GetFolderPath('ProgramFilesX86'); \
             $local = [Environment]::GetFolderPath('LocalApplicationData'); \
             $local; if ($local) { Join-Path $local 'Programs' }",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    let mut roots = vec![];
    for path in String::from_utf8_lossy(&output.stdout).lines() {
        if path.trim().is_empty() {
            continue;
        }
        let output = Command::new(wslpath)
            .args(["-u", path.trim()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        if let Ok(output) = output
            && output.status.success()
            && let Ok(path) = String::from_utf8(output.stdout)
            && !path.trim().is_empty()
        {
            roots.push(PathBuf::from(path.trim()));
        }
    }
    roots
}

fn discover_windows(paths: &[PathBuf], roots: &[PathBuf]) -> Vec<Browser> {
    let mut found = vec![];
    let browsers = [
        (
            "Google Chrome",
            "chrome.exe",
            "Google/Chrome/Application/chrome.exe",
        ),
        (
            "Microsoft Edge",
            "msedge.exe",
            "Microsoft/Edge/Application/msedge.exe",
        ),
        ("Firefox", "firefox.exe", "Mozilla Firefox/firefox.exe"),
        (
            "Brave",
            "brave.exe",
            "BraveSoftware/Brave-Browser/Application/brave.exe",
        ),
        ("Chromium", "chrome.exe", "Chromium/Application/chrome.exe"),
        ("Vivaldi", "vivaldi.exe", "Vivaldi/Application/vivaldi.exe"),
        ("Opera", "opera.exe", "Opera/launcher.exe"),
    ];
    let mut add = |name: &str, path: PathBuf| {
        if path.is_file() && !found.iter().any(|b: &Browser| b.executable == path) {
            found.push(Browser {
                name: name.into(),
                executable: path,
                args: vec![],
            });
        }
    };
    // Check install folders first so Chromium's chrome.exe keeps its identity
    // when it is also on PATH.
    for (name, _, relative) in browsers {
        for root in roots {
            add(name, root.join(relative));
        }
    }
    for (name, executable, _) in browsers {
        if name == "Chromium" {
            continue;
        }
        for root in paths {
            add(name, root.join(executable));
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

fn browser_id(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "google chrome" | "google-chrome" | "google-chrome-stable" => "chrome".into(),
        "microsoft edge" | "microsoft-edge" => "edge".into(),
        "brave-browser" => "brave".into(),
        "chromium-browser" => "chromium".into(),
        _ => name.to_ascii_lowercase(),
    }
}

/// Resolve a config override when opening, so imports remain portable.
pub fn resolve(value: &str) -> Browser {
    discover()
        .into_iter()
        .find(|browser| browser_id(&browser.name) == browser_id(value))
        .unwrap_or_else(|| Browser {
            name: value.into(),
            executable: PathBuf::from(value),
            args: vec![],
        })
}

pub fn open(browser: &Browser, url: &str) -> Result<()> {
    crate::resource::validate_url(url)?;
    // macOS uses Launch Services to address an already running app correctly.
    let mut command = if cfg!(target_os = "macos")
        // Internal schemes are browser arguments, not OS URL handlers.
        && !crate::resource::is_browser_url(url)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_ids_match_platform_browser_names() {
        for (id, names) in [
            ("firefox", vec!["Firefox", "firefox"]),
            (
                "chrome",
                vec!["Google Chrome", "google-chrome", "google-chrome-stable"],
            ),
            ("safari", vec!["Safari"]),
            ("edge", vec!["Microsoft Edge", "microsoft-edge"]),
            ("brave", vec!["Brave", "brave-browser"]),
            ("chromium", vec!["chromium", "chromium-browser"]),
            ("arc", vec!["Arc"]),
            ("vivaldi", vec!["Vivaldi", "vivaldi"]),
            ("opera", vec!["opera"]),
        ] {
            for name in names {
                assert_eq!(browser_id(name), browser_id(id), "{name}");
            }
        }
        assert_ne!(browser_id("chrome"), browser_id("chromium"));
    }

    #[test]
    fn executable_paths_preserve_case_and_spaces() {
        let path = "/custom/Browser App/bin/MyBrowser";
        let browser = resolve(path);
        assert_eq!(browser.executable, PathBuf::from(path));
        assert!(browser.args.is_empty());
    }

    #[test]
    fn windows_browsers_are_found_in_path_and_system_and_user_installations() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("Program Files");
        let system_x86 = dir.path().join("Program Files (x86)");
        let user = dir.path().join("Users/Windows User/AppData/Local");
        let programs = user.join("Programs");
        let chrome = system.join("Google/Chrome/Application/chrome.exe");
        let edge = system_x86.join("Microsoft/Edge/Application/msedge.exe");
        let firefox = programs.join("Mozilla Firefox/firefox.exe");
        let brave = user.join("BraveSoftware/Brave-Browser/Application/brave.exe");
        let chromium = user.join("Chromium/Application/chrome.exe");
        let vivaldi = user.join("Vivaldi/Application/vivaldi.exe");
        let opera = programs.join("Opera/launcher.exe");
        for path in [
            &chrome, &edge, &firefox, &brave, &chromium, &vivaldi, &opera,
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let path_only = dir.path().join("custom browser");
        std::fs::create_dir_all(path_only.join("firefox.exe")).unwrap(); // Not a file.
        std::fs::write(path_only.join("vivaldi.exe"), "").unwrap();
        let found = discover_windows(
            &[
                chrome.parent().unwrap().to_owned(),
                chrome.parent().unwrap().to_owned(),
                chromium.parent().unwrap().to_owned(),
                path_only.clone(),
            ],
            &[system, system_x86, user, programs],
        );
        assert_eq!(found.len(), 8); // Chrome is also on PATH, but appears only once.
        for (id, path) in [
            ("chrome", chrome),
            ("edge", edge),
            ("firefox", firefox),
            ("brave", brave),
            ("chromium", chromium),
            ("vivaldi", vivaldi),
            ("opera", opera),
            ("vivaldi", path_only.join("vivaldi.exe")),
        ] {
            let browser = found
                .iter()
                .find(|browser| browser.executable == path)
                .unwrap();
            assert_eq!(browser_id(&browser.name), id);
            assert!(browser.args.is_empty());
        }
        assert!(discover_windows(&[], &[dir.path().join("missing")]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn windows_roots_use_windows_folders_and_wsl_path_translation() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let powershell = dir.path().join("powershell.exe");
        let wslpath = dir.path().join("wslpath");
        std::fs::write(
            &powershell,
            r#"#!/bin/sh
printf '%s\r\n' 'C:\Program Files' 'D:\Program Files (x86)' 'C:\Users\Windows User\AppData\Local' 'C:\Users\Windows User\AppData\Local\Programs' ''
"#,
        )
        .unwrap();
        std::fs::write(&wslpath, r#"#!/bin/sh
test "$1" = -u || exit 1
case "$2" in
  'C:\Program Files') printf '%s\n' '/custom/c/Program Files' ;;
  'D:\Program Files (x86)') printf '%s\n' '/custom/d/Program Files (x86)' ;;
  'C:\Users\Windows User\AppData\Local') printf '%s\n' '/custom/c/Users/Windows User/AppData/Local' ;;
  'C:\Users\Windows User\AppData\Local\Programs') printf '%s\n' '/custom/c/Users/Windows User/AppData/Local/Programs' ;;
  *) exit 1 ;;
esac
"#).unwrap();
        for path in [&powershell, &wslpath] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(
            windows_install_roots(&powershell, &wslpath),
            [
                "/custom/c/Program Files",
                "/custom/d/Program Files (x86)",
                "/custom/c/Users/Windows User/AppData/Local",
                "/custom/c/Users/Windows User/AppData/Local/Programs",
            ]
            .map(PathBuf::from)
        );
        let missing = dir.path().join("missing");
        assert!(windows_install_roots(&missing, &wslpath).is_empty());
        assert!(windows_install_roots(&powershell, &missing).is_empty());
        std::fs::write(&powershell, "#!/bin/sh\nexit 1\n").unwrap();
        assert!(windows_install_roots(&powershell, &wslpath).is_empty());
    }
}
