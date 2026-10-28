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
Current version is 0.10.3 but any minor version can ship with a breaking change until v1.**

### Version 1.0.0 goals 

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
| `qrlkit nuke` | Reset imports, renames, browser selection, hooks, and theme settings |
| `qrlkit themes` | List themes with previews |
| `qrlkit themes current` | Show the global theme and tool policy |
| `qrlkit themes add <file>` | Import a theme without activating it |
| `qrlkit themes set [name]` | Set the global theme; omit name for live previews |
| `qrlkit themes tools <on/off>` | Enable or disable tool themes |
| `qrlkit themes tools deny <tool>` | Ignore a tool's theme |
| `qrlkit themes tools reset <tool>` | Remove a tool's theme exclusion |
| `qrlkit --help` | Show help |

`nuke` keeps your original config files and installed shell integration.
State is stored in `$XDG_STATE_HOME/qrlkit/state.yaml` on Linux, falling back to
`~/.local/state/qrlkit/state.yaml`, or in
`~/Library/Application Support/qrlkit/state.yaml` on macOS. Use
`qrlkit --config <path> …` for separate state (usefull for debugging). 

## Config syntax

### URLs

Open URLs in your default browser.

```toml
[repos]
rs = "https://github.com/qrlkit/qrlkit-rs"
```

Or go advanced with reuseable constants and hints.

```toml
[const]
hidden = true
org = "qrlkit"
repos = ["qrlkit-rs", "qrlkit-web"]

[repos.open]
url = "https://github.com/{const.org}/{const.repos}"
hint = "Browse the source code"
browser = "firefox"
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

Or beef it up with reuseable constants, hints and a custom filehook: 

```toml
[const]
hidden = true
root = "~/dev"
repos = ["qrlkit-rs", "qrlkit-web"]

[files.readme]
url = "{const.root}/{const.repos}/README.md"
hint = "Edit the project README"
filehook = "nvim $file"
```

```sh
qrlkit files readme qrlkit-rs  # Open directly
qrlkit files readme            # Choose interactively
```

### Dirs

By default you can `cd` into a dir: 

```toml
[dirs]
project = "~/dev/qrlkit-rs"
```

Or mod it similarly to files and urls with consts, dirhooks and hints:  

```toml
[const]
hidden = true
root = "~/dev"
repos = ["qrlkit-rs", "qrlkit-web"]

[dirs.project]
url = "{shared.root}/{shared.repos}"
hint = "Enter and inspect the project"
dirhook = "cd $dir && tree -L 1"
```

```sh
qrlkit dirs project qrlkit-rs  # Open directly
qrlkit dirs project            # Choose interactively
```

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

Supply fixed values through `env` without prompting.

```toml
[shared]
hidden = true
name = "World"

[greet]
run = 'printf "Hello, %s!\n" "$NAME"'

[greet.env]
NAME = "{shared.name}"
```

Combine shared values with argument choices.

```toml
[shared]
hidden = true
name = "World"
greetings = ["Hello", "Hi", "Hey"]

[greet]
args = [
  { name = "greeting", enum = { ref = "shared.greetings" } },
]
run = 'printf "%s, %s!\n" "$1" "$NAME"'

[greet.env]
NAME = "{shared.name}"
```

Add hints and choose your shell.

```toml
[shared]
hidden = true
name = "World"
greetings = ["Hello", "Hi", "Hey"]

[greet]
hint = "Say hello with some personality"
shell = "zsh"
args = [
  { name = "greeting", enum = { ref = "shared.greetings" } },
]
run = '''
printf '%s, %s!\n' "$1" "$NAME"
printf 'Have a lovely day!\n'
'''

[greet.env]
NAME = "{shared.name}"
```

```sh
qrlkit greet Hey
```

Supported shells: `bash`, `sh`, and `zsh`.

### Shared values

Put shared values in any config and register it with `qrlkit add`.

```toml
# shared.toml
[shared]
hidden = true
root = "~/dev"
name = "World"
```

```sh
qrlkit add shared.toml
```

Reference them from other registered configs.

```toml
# QRLs.toml
[dirs]
project = "{shared.root}/{repo}"

[greet]
run = 'printf "Hello, %s!\n" "$NAME"'

[greet.env]
NAME = "{shared.name}"
```

### Composition

Reference another resource's target without opening it.

```toml
# Reference another QRL's value or URL
url = "{project.home}/issues"
```

```toml
# Use a script's output
[branch]
run = "git branch --show-current"

[compare]
url = "{project.home}/compare/main...{branch}"
```

```toml
# Look up a value using one selector
[project]
environment = ["staging", "production"]

[open]
url = "https://{hosts[project.environment]}.example.com"
```

```toml
# Walk a nested mapping using multiple selectors
[project]
name = ["api", "web"]
environment = ["staging", "production"]

[open]
url = "https://{hosts[project.name, project.environment]}.example.com"
```

Missing references and circular dependencies are errors.

### Hints and hidden groups

Hide a group from pickers with `hidden = true`. Its values can still be referenced
or called directly. Add hints to groups, tools, lists, and individual choices.

```toml
[projects]
hidden = true

[projects.repos]
hint = "Choose a qrlkit project"
values = [
  { value = "qrlkit-rs", hint = "Rust CLI" },
  { value = "qrlkit-web", hint = "Documentation website" },
]

[repos]
hint = "GitHub resources"

[repos.open]
hint = "Browse the repository"
url = "https://github.com/qrlkit/{projects.repos}"
```

```sh
repos open       # Choose a repository with hints
projects repos   # Choose and print a value from the hidden group
```

Use `values = ["qrlkit-rs", "qrlkit-web"]` when only the list needs a hint.
Each tool keeps its own hint when composing resources.

See [examples/composition.toml](examples/composition.toml) for a complete config.

## Themes

Make the picker fit your terminal.

Available themes:
`terminal` (default), `catppuccin-latte`, `catppuccin-mocha`, `dracula`,
`gruvbox-dark`, `gruvbox-light`, `nord`, and `tokyo-night`.

Preview the themes and choose a global default.

```sh
qrlkit themes              # List themes with previews
qrlkit themes set          # Preview as you navigate; Enter saves, Esc cancels
qrlkit themes set dracula  # Choose directly
qrlkit themes current      # Show your theme and tool policy
```

Give a tool its own theme in the config.

```toml
[gh]
theme = "dracula"
home = "https://github.com"
```

Or choose the colors, box and selection marker yourself.

```toml
[gh]
home = "https://github.com"

[gh.theme]
background = "#1e1e2e"
text = "#cdd6f4"
selected = "#ffffff"
selected_background = "#45475a"
hint = "#a6adc8"
title = "#cba6f7"
border = "rounded"         # none, plain, rounded, or double
border_color = "#9399b2"
marker = "--x"
selection_width = "row"     # Or content: highlight only the marker, key and hint
```

Nested keys keep their parent's theme unless they define their own.
A new theme replaces the whole thing. Missing values use terminal colors,
no border, a `>` marker and full-row selection. Themes are never mixed.
Use `"default"` for a color to restore your terminal's color.

Save your own global theme in a separate file with a name inside `[theme]`.

```toml
# personal-theme.toml
[theme]
name = "personal"
background = "#1e1e2e"
text = "#cdd6f4"
selected = "#cba6f7"
selected_background = "#45475a"
border = "rounded"
```

```sh
qrlkit themes add personal-theme.toml
qrlkit themes set personal
```

Importing saves a copy and adds it to the list.

Tool themes are on by default. Turn them off to use your global theme everywhere,
or exclude a single tool and all its nested keys.

```sh
qrlkit themes tools off      # Use your global theme everywhere
qrlkit themes tools on       # Use tool themes where defined
qrlkit themes tools deny gh  # Use your global theme for gh
qrlkit themes tools reset gh # Remove gh's exclusion
```

Tools without a theme use your global default. Exclusions survive off/on toggles.
`qrlkit nuke` resets the theme to `terminal`, enables tool themes and removes
exclusions and imported themes.

## Contributing

Contributions are welcome! 
Either drop a PR or create an issue. 


[MIT licensed](LICENSE).
