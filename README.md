<p align="center">
  <img src="QRLs.png" alt="QRLs" width="500">
</p>

[![CI](https://github.com/qrlkit/qrlkit-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/qrlkit/qrlkit-rs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/qrlkit.svg)](https://crates.io/crates/qrlkit)
[![Documentation](https://img.shields.io/badge/docs-qrlkit.github.io-blue.svg)](https://qrlkit.github.io/qrlkit-web/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

# Quick Resource Locators (QRLs)

Basic tool to keep your key resources quickly available at your fingertips. 

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
"""
shell = "zsh"

[qk.dirs]
rs = "~/dev/qrlkit-rs"
web = "~/dev/qrlkit-web"
dirhook = "cd $dir && tree -L 1"

[files]
cargo-toml = "~/dev/qrlkit-rs/Cargo.toml"
readme = "~/dev/qrlkit-rs/README.md"
changelog = "~/dev/qrlkit-web/CHANGELOG.md"
filehook = "nvim $file"
```

Import the file with `qrlkit add` and turn it in to a lightweight CLI tool:

```sh
$ qrlkit add QRLs.toml

# Open a new terminal, then:

repos rs            # Opens gh repo with browser
repos prs web       # Opens prs for gh page in browser
qk check            # Run standard rust checks and tests in chosen shell
qk dirs web         # cd into dir and see content via custom dirhook
qk files readme     # open qrlkit-rs README.md in neovim

```

The tool is built to 1) optimize for adaptability to *your* flow:

- `qrlkit add <x>` accepts any file name and supports toml, json and yaml configs
- Set hooks to customize behavior for files and dirs in each config and/or globally
- Define your own CLI behavior:
    - By setting config keys you decide if it's `logs live nginx` or `logs nginx live`. 
    - Name tools and paths what you want
- Multiple configs are supported at once. Make one for your projects, your machine or your team/org

and 2) have as little friction as possible:

- If you forget something run `qrlkit` to run any imported tool
- `qrlkit` will suggest the next keys if you get stuck
- you don't need to do the `qrlkit init`. Tool will ask for defaults when needed. 

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

## Version 1 Roadmap

**Version 1 happens January 2027. Until then expect everything to break. 
Current version is 0.5.5 but any minor version can ship with a breaking change until v1.**

### Version 1

#### `qrlkit` ####

- [x] init
- [x] add <file | dir path>
    - [x] multiple configs at once
    - [x] graceful collisions
- [x] qrlkit [keys…]
- [x] ls
- [x] rm <path>
- [x] reload
- [x] set-browser
- [x] set-filehook <command>
- [x] set-dirhook <command>
- [x] nuke
- [x] set-collision-strategy <rename | merge> 

#### config ####

- [x] yaml, json and toml
- [x] nested keys
- [x] URLs
- [x] files and dirs
- [x] shell scripts 
- [x] variables in paths, urls, scripts
- [x] file hooks
- [x] dir hooks
- [x] browser hooks
- [x] run shell scripts
- [x] automatic hints
- [x] hardcoded arg suggestions
- [x] constants 

#### package managers ####

- [x] `cargo install`
- [x] `npm install`

#### Raw install #### 

- [x] linux and macos binaries released on GitHub
- [x] install.sh 

## Commands

| Command | What it does |
| --- | --- |
| `qrlkit [keys…]` | Browse or open a resource |
| `qrlkit --config <path> …` | Use a separate state file for debugging |
| `qrlkit add <file-or-directory>` | Register configs and set up root commands |
| `qrlkit ls` | List registered config files |
| `qrlkit rm <path>` | Unregister a config; keep the file |
| `qrlkit reload` | Refresh imports and retry shell setup |
| `qrlkit set-collision-strategy <strategy>` | Choose `merge` (default) to combine shared namespaces or `rename` to rename conflicting roots |
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
`qrlkit --config <path> …` for separate state (usefull for debugging). 

## Config syntax

**URLs**

Urls open in the default browser set in `qrlkit init` or `qrlkit set-browser`. 
Reserved key `browser` overrides it with options:

- `chrome` 
- `firefox`
- `safari`
- `edge`
- `brave`
- `chromium`
- `arc`
- `vivaldi`
- `opera`

Executable paths are also supported. 
Use `{}` to forward arguments to paths. 

```toml
[docs]
browser = "firefox"
fav = "https://some-docs-you-prefer-in-ff/{area}.com"
```

**files**

Files use the default action set in `qrlkit init` or `qrlkit set-filehook`.
If neither is used they just otherwise print the file path.
Reserved key `filehook` overrides global behavior with a command using `$file` for the selected path.
Use `{}` in paths to forward arguments. 

```toml
[dotfiles]
filehook = "nvim $file"
edit = "~/.confg/{tool}/{file}"
```

**dirs**

Dirs use the default action set in `qrlkit init` or `qrlkit set-dirhook`.
If neither is used they otherwise just `cd` into directory via shell integration.
Reserved key `dirhook` overrides global behavior with a command using `$dir` for the selected path.

```toml
[dirs]
dirhook = "cd $dir && tree -L 1"
project = "~/dev/{repo}"
```

**constants**

Define reusable strings or lists in `[constants]`. Use `{constants.name}` in paths and URLs. For shared values, put `[constants]` in `constants.toml` beside the state file and use `{global.name}`.

```toml
[constants]
root = "~/dev"
namespaces = ["dev", "staging", "prod"]

[projects]
api = "{constants.root}/api"
```

**scripts**

Scripts run in Bash by default from the config's directory.
Reserved key `shell` overrides it with options `bash`, `sh`, and `zsh`.
Use `$` notation to pass arguments. Use args to prompt users for inputs.

```toml
[kube.logs]
args = [
  { name = "namespace", enum = { ref = "constants.namespaces" } },
  { name = "pod" },
]
run = '''
kubectl logs --namespace "$1" "$2" --follow
'''
shell = "zsh"
```

**hints**

Comments with `hint: ` in them will be included in the terminal.

```toml
[launch]
# hint: Find your next mission
issues = "https://github.com/ratatui/ratatui/issues"

# hint: Browse the source code
repo = "https://github.com/ratatui/ratatui"
```

Run `qrlkit launch` after importing the config:

```text
> issues        Find your next mission
  repo          Browse the source code
```


## Contributing

Contributions are welcome! 
Either drop a PR or create an issue. 


[MIT licensed](LICENSE).
