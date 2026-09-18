# Quick Resource Locators (qrlkit)

A config-file-to-CLI runtime written in Rust. Organize links, directories, files,
and scripts into TOML, YAML, or JSON, then run them from your terminal.

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

**Every top-level resource name becomes a command automatically.** No `[alias]`
section is needed. `docs handbook` means `qrlkit docs handbook`, and `notes edit`
means `qrlkit notes edit`. Run `docs` to browse only its resources, or `qrlkit`
to browse all imported resources. Use **↑/↓** or **j/k**, **Enter** to select,
and **Esc** to cancel.

Import sets up Bash, Zsh, Fish, or PowerShell integration in your shell startup
file, preserving its existing contents and backing it up before the first change.
Open a new terminal to activate commands. A child process cannot install functions
into the shell that launched it.

## Files and directories

```toml
[infra.dir]
manifests = "~/work/platform/manifests"

[infra.file]
kubeconfig = "~/.kube/config"
```

```sh
infra dir manifests
config=$(infra file kubeconfig)
```

Relative resource paths resolve from your shell’s current directory. Use an
absolute path or `~/` for shortcuts to a fixed location.

## File hooks

Files print their absolute path by default. Choose a command to open them instead:

```sh
qrlkit set-filehook "nvim file && echo Done"
notes today  # Opens the file in Neovim
qrlkit set-filehook --clear  # Restore path printing
```

The preference is saved across sessions. A `"$filehook"` in a config overrides it;
put it before any tables for a file-wide default, or inside a group for a more
specific override. The nearest setting wins. An empty string prints the path.

```toml
"$filehook" = "nvim file"

[notes]
today = "~/notes/today.txt"

[notes.preview]
"$filehook" = "less file"
today = "~/notes/today.txt"

[notes.paths]
"$filehook" = ""
today = "~/notes/today.txt"
```

Each standalone, **unquoted** `file` word is replaced with the safely quoted
absolute path, including paths containing spaces. Quote executable paths or other
arguments containing spaces, e.g. `qrlkit set-filehook '"/path to/editor" file'`.
Like directory hooks, file hooks run as shell code in Bash on macOS/Linux or
PowerShell 7 on Windows. Shell operators such as `&&`, `||`, pipes, and redirects
work, as do variables and command substitution using that shell's syntax. Use
single quotes around the command at setup time to defer `$` expansion until the
hook runs. Hooks run in your current directory, inherit the terminal, and return
the shell's exit status. Bash hooks stop on unhandled failures and enable
`pipefail`. They apply only to
files; directories still change directory and URLs still open in your browser.
Local hooks reload automatically and also work in YAML and JSON.

## Directory hooks

Directories change your terminal's working directory by default. Set a hook to
run additional commands:

```sh
qrlkit set-dirhook "cd dir && tree -L 1 ."
# Open a new terminal to load the updated shell integration, then:
docs repo
qrlkit set-dirhook --clear  # Restore the default directory change
```

Use `"$dirhook"` to override the saved default for a config or nested group.
The nearest setting wins; an empty string restores the ordinary directory change.

```toml
"$dirhook" = "cd dir && tree -L 1 ."

[projects]
api = "~/work/api"

[projects.quiet]
"$dirhook" = ""
api = "~/work/api"
```

The standalone, **unquoted** `dir` word is replaced with the safely quoted absolute
path. Hooks run as shell code in Bash on macOS/Linux or PowerShell 7 on Windows,
so operators such as `&&` work. Use syntax appropriate for that shell. Hooks start
in your current directory; include `cd dir` when you want to enter the target.
Their final directory is applied to your terminal through shell integration,
even if a command after `cd` fails. The hook's exit status is preserved. Other
child-shell changes, such as variables and aliases, do not persist in your terminal.
Hooks affect directories only and also work in YAML and JSON.

## Scripts

```toml
[infra.it]
"$run" = '''
namespace="$1"
context="$2"
pod="$3"
kubectl --namespace "$namespace" --context "$context" exec -it "$pod" -- sh
'''
"$shell" = "bash"

[infra.pr]
"$run" = 'gh pr create "$@"'
```

```sh
infra it payments staging api-server-abc123
infra pr --title "Ship it" --draft
```

Arguments arrive in order as `$1`, `$2`, `$3`; use `"$@"` to forward all of them.
Scripts start in the config file’s directory. `"$shell"` is optional: Bash is the
default on macOS/Linux, PowerShell 7 on Windows. PowerShell scripts use `$args`.
A top-level script also becomes a command directly:

```toml
[deploy-preview]
"$run" = 'echo "Deploying $1"'
```

Run `deploy-preview my-branch` after importing and opening a new terminal.

## Fill in the blanks

```toml
[logs]
request = "https://logs.example.com/{env}/requests/{id}"

[infra.dir]
repo = "~/work/{project}"
```

```sh
logs request prod abc123
infra dir repo payments
```

Arguments fill `{names}` in order of first appearance. Repeat a name to reuse its
value. Leave arguments out and qrlkit asks for them—even when browsing the menu.
Extra arguments return an error. URL values are percent-encoded; path values stay
literal. Names use letters, digits, or underscores; braces mark placeholders.

## Many configs, many formats

```sh
qrlkit add org-guides.toml
qrlkit add devops-magic.yaml
qrlkit add frontend-tools.json
qrlkit add ~/qrl-configs
```

The file extension selects the format; `.yml` is supported too.
See [examples](examples/) for matching TOML, YAML, and JSON configurations.

Pass a directory to import all `.toml`, `.yaml`, `.yml`, and `.json` files directly
inside it, in filename order. Extensions are case-insensitive; other files and
subdirectories are ignored. Already registered files are skipped. The whole batch
is validated and namespace collisions are resolved before saving, so an invalid
file or unresolved collision leaves your imports unchanged. A directory with no
supported files reports an error.

Files are registered individually; the directory is not watched. Run `qrlkit add
<directory>` again to pick up new files, or `qrlkit reload` to refresh existing ones.

Edit a file and carry on: resources reload automatically before use. After adding
or renaming top-level commands, run `qrlkit reload` and open a new terminal.
Removing a config with `qrlkit rm`, deleting a root and reloading, or running
`qrlkit nuke` removes its commands from future shell sessions. Existing terminals
keep their functions until closed.

### Name collisions

Two files using the same root key? qrlkit asks you to rename one namespace:

```text
Import collision — choose the namespace to rename
> /work/team-one.toml: qrlkit logs
  /work/team-two.toml: qrlkit logs

Rename qrlkit logs: enter a new root, e.g. team2-logs
team1-logs
```

The renamed root becomes the command (`team1-logs …`). Every child follows the
rename, qrlkit remembers it on reload, and the original files stay untouched.

Automatic command names must start with an ASCII letter and contain only letters,
digits, `-`, or `_`. Invalid names and reserved shell names are skipped with a
message; resources remain accessible through `qrlkit <root> …`. Existing shell
commands, aliases, and functions are never overwritten: conflicts are reported
when the terminal starts. Rename the root in your file to choose another name.
Names that differ only by case cannot both become automatic commands.

### Existing explicit aliases

Legacy `[alias] name = "tool-name"` metadata remains supported in TOML, as does
`alias.name` in YAML and JSON. These aliases browse their entire source file and
keep its original resource names. Explicit aliases take precedence over automatic
root commands with the same name. Duplicate explicit aliases and existing command
names are rejected. New configurations should use top-level resource names.

## Install

With Rust and Cargo installed:

```sh
cargo install --git https://github.com/jakobhautop/QRL.git --locked
```

Cargo installs `qrlkit` into its bin directory (normally `~/.cargo/bin`). Make sure
that directory is on your `PATH`. Run the same command again to install updates.

Upgrading from `qrl`? Run `qrlkit reload` to update managed shell integration,
then open a new terminal. Existing imports and settings are preserved. Manually
configured shell initialization should use `qrlkit init` instead of `qrl init`.

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
