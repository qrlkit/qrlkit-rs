![](QRLs.png)

# Quick Resource Locators (QRLs)

QRLs are small shortcuts you define for your key bookmarks, 
files, directories, scripts etc. Simply type them into `.toml`, 
`.yaml` or '.json' files and `qrlkit` converts them to cli-tools. 
Its quick and personalizeable. 

```toml
# resources.toml
[docs]
handbook = "https://example.com/handbook"
repo = "~/work/project"

[notes]
today = "~/notes/today.txt"

[notes.edit]
"$run" = 'nvim ~/notes/today.txt'
```

Import the file once:

```sh
qrlkit add resources.toml
# Open a new terminal, then:
docs handbook       # Opens the URL
docs repo           # Changes your working directory
notes today         # Prints the absolute file path
notes edit          # Runs the script
```

## Install

*Step 1* Clone repo and install qrlkit binary 

```
git clone git@github.com:qrlkit/qrlkit-rs.git
cd qrlkit-rs
cargo install --path . 
```

*Step 2* Write a toml, yaml or json config

```
[repos]
qrlkit = 'https://github.com/qrlkit/qrlkit-rs'
```

*Step 3* Run interactive setup

```
qrlkit init
```

Choose an installed browser, then a supported shell, filehook command, and
dirhook command. Press Enter for the detected shell (Bash if detection fails on
Unix, PowerShell on Windows), printing file paths, and changing directories.
Custom hooks use the unquoted `file` or `dir` placeholder. Setup saves your
preferences and installs shell integration; open a new terminal to activate it.
Import your config with `qrlkit add <file-or-directory>`.

## Features

- Keys are turned in cli-tool commands
- Supports toml, yaml, json
- Open browser settings and extension management using internal URLs.

- Defaults:
    - URLs opens in default browser
    - Dir paths are cd'd into
    - File paths are printed in stdout
    - Scripts are executed with bash

- Customize filehook, dirhook, shell and browser
- Urls and paths can be modified with user inputs
- Scripts can also use inputs
- Name collisions are handled by qrlkit cli on import 

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
State stays in `~/.config/qrl/state.yaml`, or `$XDG_CONFIG_HOME/qrl/state.yaml`,
for compatibility with existing installations. Use `qrlkit --config <path> …`
for separate state. Put qrlkit options before resource keys; arguments after a
script’s keys belong to the script.

## Contributing

Built with Rust and Ratatui. To check a change:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/test-terminal.py # macOS / Linux
```

[MIT licensed](LICENSE).
