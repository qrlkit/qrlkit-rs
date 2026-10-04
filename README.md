![](QRLs.png)

[![CI](https://github.com/qrlkit/qrlkit-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/qrlkit/qrlkit-rs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/qrlkit.svg)](https://crates.io/crates/qrlkit)
[![Documentation](https://img.shields.io/badge/docs-qrlkit.github.io-blue.svg)](https://qrlkit.github.io/qrlkit-web/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

# Quick Resource Locators (QRLs)

Basic tool to keep your key resources quickly availble at your fingertips. 

Define a config with urls, dirs, scripts/snippets, files etc:  

```toml
# QRLs.toml

[repos]
rs = "https://github.com/qrlkit/qrlkit-rs"
web = "https://github.com/qrlkit/qrlkit-web"

[repos.prs]
rs = "https://github.com/qrlkit/qrlkit-rs"
web = "https://github.com/qrlkit/qrlkit-web/pulls"

[qk.check]
run = """
cargo fmt
cargo test 
cargo clippy
python3 "~dev/qrlkit-rs/test/integration.py"
""
shell = "zsh"

[qk.dirs]
rs = "~/dev/qrlkit-rs"
web = "~/dev/qrlkit-web"
dirhook = "cd $dir && tree -L 1 && 

[files]
cargo-toml = "~/dev/qrlkit-rs/Cargo.toml"
readme = "~/dev/qrlkit-rs/README.md"
changelog = "~/dev/qrlkit-web/CHANGELOG.md"
filehook = "nvim $file"
```

Import the file once:

```sh
qrlkit add QRLs.toml
# Open a new terminal, then:

repos rs            # Opens gh repo with browser
repos prs web       # Opens prs for gh page in browser
qk check            # Run standard rust checks and tests in chosen shell
qk dirs web         # cd into dir and see content via custom dirhook
qk files readme     # open qrlkit-rs README.md in neovim

```

Tool is build to optimize for adaptability to *your* flow so:

- `qrlkit add <x>` accepts any file name and supports toml, json and yaml configs
- Set custom files and dir hooks in each config and use `--filehook` 
`--dirhook` `---browser` to global defaults
- Define your own cli behavior:
    - By settings config keys you decide if its `logs live nginx` or `logs nginx live`. 
    - Name tools and paths what you want
- Multiple configs are supported at once. Make one for your projects, your machine or your team/org


## Install

*Step 1* Clone repo and install qrlkit binary 

```
git clone git@github.com:qrlkit/qrlkit-rs.git
cd qrlkit-rs
cargo install --path .

# Then start init to set the defaults you want (shell, browser etc)
qrlkit init 
```

*Step 2* Write some config you want 

*Step 3* Add the config

```
qrlkit add shortcuts.toml
```

```

## Version 1 Roadmap

**Version 1 happens January 2027. Until then expect everything to break. 
Current version is 0.5.1 but any minor version can ship with a breaking change until v1.**

### Version 1

- `qrlkit`
    - [x] init
    - [x] add <file | dir path>
        - [x] multiple configs at once
        - [x] gracefull collisions

- config
    - [x] yaml, json and toml
    - [x] nested keys
    - [x] URLs
    - [x] files and dirs
    - [x] shell scripts 
    - [x] file hooks
    - [x] dir hooks
    - [ ] browser hooks
    - [ ] Run shell scripts 

- platforms/install
    - package managers
    - [x] `cargo install`
    - [ ] ´npm install´

    - Raw install 
        - [x] linux and macos binaries released on github
        - [ ] install.sh 

## Commands

| Command | What it does |
| --- | --- |
| `qrlkit [keys…]` | Browse or open a resource |
| `qrlkit add <file-or-directory>` | Register configs and set up root commands |
| `qrlkit ls` | List registered config files |
| `qrlkit rm <path>` | Unregister a config; keep the file |
| `qrlkit reload` | Refresh imports and retry shell setup |
| `qrlkit set-browser` | Choose the default browser |
| `qrlkit set-filehook <command>` | Choose the default file action (`--clear` restores path printing) |
| `qrlkit set-dirhook <command>` | Choose the directory hook (`--clear` restores changing directory) |
| `qrlkit init` | Choose browser, shell, filehook, and dirhook interactively |
| `qrlkit init <shell>` | Print the directory wrapper for manual setup |
| `qrlkit nuke` | Reset imports, renames, browser selection, and hooks |
| `qrlkit --help` | Show help |

`nuke` keeps your original config files and installed shell integration.
State is stored in `$XDG_STATE_HOME/qrlkit/state.yaml` on Linux, falling back to
`~/.local/state/qrlkit/state.yaml`, or in
`~/Library/Application Support/qrlkit/state.yaml` on macOS. Use
`qrlkit --config <path> …` for separate state. Put qrlkit options before
resource keys; arguments after a script’s keys belong to the script.

## Contributing

Built with Rust and Ratatui. To check a change:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/test-terminal.py # macOS / Linux
```

[MIT licensed](LICENSE).
