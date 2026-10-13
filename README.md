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
rs = "https://github.com/qrlkit/qrlkit-rs/pulls"
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

### From package manager  

A few package managers can be used to install the binary for your platform:

```
cargo install qrlkit

npm i -g qrlkit

bun add -g qrlkit

yarn global add qrlkit

```

### From Github release:

Install the latest release for Linux or macOS into `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/qrlkit/qrlkit-rs/main/install.sh | sh
```

### From Source  

Clone repo and install qrlkit binary with cargo  

```
git clone git@github.com:qrlkit/qrlkit-rs.git
cd qrlkit-rs
cargo install --path .
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
- [x] rm [keys…]
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
| `qrlkit rm [keys…]` | Remove a key and everything under it; omit keys to choose |
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

Each example replaces the previous one.

Resource arguments match every placeholder name.
CLI values follow argument declaration order.

### URLs

Open URLs in your default browser.

```toml
[repos]
open = "https://github.com/qrlkit/qrlkit-rs"
```

Choose the repository with an argument.

```toml
[repos]
open = "https://github.com/qrlkit/{repo}"
```

Reuse an organization through a constant.

```toml
[constants]
org = "qrlkit"

[repos]
open = "https://github.com/{constants.org}/{repo}"
```

Add hints and override the browser.

```toml
[constants]
org = "qrlkit"

[repos]
browser = "firefox"
# hint: Browse the source code
open = "https://github.com/{constants.org}/{repo}"
```

Offer repository choices from a constant.

```toml
[constants]
org = "qrlkit"
repos = ["qrlkit-rs", "qrlkit-web"]

[repos.open]
url = "https://github.com/{constants.org}/{repo}"
hint = "Browse the source code"
browser = "firefox"
args = [
  { name = "repo", enum = { ref = "constants.repos" } },
]
```

```sh
qrlkit repos open qrlkit-rs  # Open directly
qrlkit repos open            # Choose interactively
```

Supported browsers:
`chrome`, `firefox`, `safari`, `edge`, `brave`, `chromium`, `arc`, `vivaldi`, `opera`.

Custom browser executable paths also work.

### Files

Print file paths without configured hooks.

```toml
[files]
readme = "~/dev/qrlkit-rs/README.md"
```

Choose the project with an argument.

```toml
[files]
readme = "~/dev/{repo}/README.md"
```

Reuse your development directory across paths.

```toml
[constants]
root = "~/dev"

[files]
readme = "{constants.root}/{repo}/README.md"
```

Add hints and open your editor.

```toml
[constants]
root = "~/dev"

[files]
filehook = "nvim $file"
readme = "{constants.root}/{repo}/README.md" # hint: Edit the project README
```

Offer project choices before opening files.

```toml
[constants]
root = "~/dev"

[files.readme]
url = "{constants.root}/{repo}/README.md"
hint = "Edit the project README"
filehook = "nvim $file"
args = [
  { name = "repo", enum = ["qrlkit-rs", "qrlkit-web"] },
]
```

```sh
qrlkit files readme qrlkit-rs  # Open directly
qrlkit files readme            # Choose interactively
```

Set global defaults with `qrlkit set-filehook`.

### Dirs

Change directories through the shell integration.

```toml
[dirs]
project = "~/dev/qrlkit-rs"
```

Choose the project with an argument.

```toml
[dirs]
project = "~/dev/{repo}"
```

Reuse your development directory across projects.

```toml
[constants]
root = "~/dev"

[dirs]
project = "{constants.root}/{repo}"
```

Add hints and list directory contents.

```toml
[constants]
root = "~/dev"

[dirs]
dirhook = "cd $dir && tree -L 1"
# hint: Enter and inspect the project
project = "{constants.root}/{repo}"
```

Offer project choices before changing directories.

```toml
[constants]
root = "~/dev"

[dirs.project]
url = "{constants.root}/{repo}"
hint = "Enter and inspect the project"
dirhook = "cd $dir && tree -L 1"
args = [
  { name = "repo", enum = ["qrlkit-rs", "qrlkit-web"] },
]
```

```sh
qrlkit dirs project qrlkit-rs  # Open directly
qrlkit dirs project            # Choose interactively
```

Set global defaults with `qrlkit set-dirhook`.

### Scripts

Run Bash from the config's directory.

```toml
[greet]
run = 'printf "Hello, World!\n"'
```

Prompt for missing arguments with `args`.

```toml
[greet]
args = [{ name = "name" }]
run = 'printf "Hello, %s!\n" "$1"'
```

Supply constants through `env` without prompting.

```toml
[constants]
name = "World"

[greet]
run = 'printf "Hello, %s!\n" "$NAME"'

[greet.env]
NAME = "{constants.name}"
```

Combine constant values with argument choices.

```toml
[constants]
name = "World"
greetings = ["Hello", "Hi", "Hey"]

[greet]
args = [
  { name = "greeting", enum = { ref = "constants.greetings" } },
]
run = 'printf "%s, %s!\n" "$1" "$NAME"'

[greet.env]
NAME = "{constants.name}"
```

Add hints and choose your shell.

```toml
[constants]
name = "World"
greetings = ["Hello", "Hi", "Hey"]

[greet]
hint = "Say hello with some personality"
shell = "zsh"
args = [
  { name = "greeting", enum = { ref = "constants.greetings" } },
]
run = '''
printf '%s, %s!\n' "$1" "$NAME"
printf 'Have a lovely day!\n'
'''

[greet.env]
NAME = "{constants.name}"
```

```sh
qrlkit greet Hey
```

Supported shells: `bash`, `sh`, and `zsh`.

### Shared constants

Place `constants.toml` beside your state file.

```toml
# constants.toml
[constants]
root = "~/dev"
name = "World"
```

Reference shared constants with `{global.name}` syntax.

```toml
# QRLs.toml
[dirs]
project = "{global.root}/{repo}"

[greet]
run = 'printf "Hello, %s!\n" "$NAME"'

[greet.env]
NAME = "{global.name}"
```

## Contributing

Contributions are welcome! 
Either drop a PR or create an issue. 


[MIT licensed](LICENSE).
