#!/bin/sh
# Install the latest release:
#   sh install.sh
# Override the destination with QRLKIT_INSTALL_DIR (default: ~/.local/bin).
# Set QRLKIT_NO_MODIFY_PATH=1 to leave shell startup files unchanged.
set -eu

fail() {
    printf 'qrlkit installer: %s\n' "$*" >&2
    exit 1
}

setup_path() {
    [ "${QRLKIT_NO_MODIFY_PATH:-0}" != 1 ] || return 0
    case ":$PATH:" in
        *:"$destination":*) return 0 ;;
    esac

    # Quote the literal path so spaces and shell metacharacters remain data.
    quoted=$(printf '%s' "$destination" | sed "s/'/'\\\\''/g")
    activate="export PATH='$quoted':\$PATH"
    path_line="case :\$PATH: in *:'$quoted':*) ;; *) $activate ;; esac"
    shell=${SHELL:-}
    case "${shell##*/}" in
        zsh) startup=${ZDOTDIR:-$HOME}/.zshrc ;;
        bash)
            startup=$HOME/.bashrc
            if [ "$os" = macos ]; then
                startup=$HOME/.bash_profile
                for profile in .bash_profile .bash_login .profile; do
                    if [ -f "$HOME/$profile" ]; then
                        startup=$HOME/$profile
                        break
                    fi
                done
            fi
            ;;
        fish)
            startup=${XDG_CONFIG_HOME:-$HOME/.config}/fish/config.fish
            quoted=$(printf '%s' "$destination" | sed "s/\\\\/\\\\\\\\/g; s/'/\\\\'/g")
            activate="fish_add_path --path '$quoted'"
            path_line=$activate
            ;;
        *)
            printf 'Add this directory to your shell PATH: %s\n' "$destination"
            return
            ;;
    esac
    if [ ! -f "$startup" ] || ! grep -Fqx -- "$path_line" "$startup"; then
        mkdir -p "$(dirname "$startup")"
        printf '\n# qrlkit PATH\n%s\n' "$path_line" >> "$startup"
        printf 'Updated PATH in %s\n' "$startup"
    fi
    printf 'Restart your terminal to use qrlkit, or run:\n  %s\n' "$activate"
}

main() {
    [ "$#" -le 1 ] || fail 'Usage: sh install.sh'
    case "${1:-}" in
        -h|--help)
            printf '%s\n' 'Usage: sh install.sh' \
                'Installs the latest release.' \
                'Set QRLKIT_INSTALL_DIR to override ~/.local/bin.' \
                'Set QRLKIT_NO_MODIFY_PATH=1 to skip shell PATH setup.'
            return
            ;;
    esac
    [ "$#" -eq 0 ] || fail 'Usage: sh install.sh'

    for command in curl tar mktemp uname; do
        command -v "$command" >/dev/null 2>&1 || fail "Required command not found: $command"
    done
    if command -v sha256sum >/dev/null 2>&1; then
        checksum=sha256sum
    elif command -v shasum >/dev/null 2>&1; then
        checksum=shasum
    else
        fail 'Install sha256sum or shasum to verify the download.'
    fi

    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        *) fail 'Supported systems: Linux and macOS.' ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) fail 'Supported architectures: x86_64 and ARM64.' ;;
    esac

    releases=https://github.com/qrlkit/qrlkit-rs/releases
    latest=$(curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
        --output /dev/null --write-out '%{url_effective}' "$releases/latest")
    case "$latest" in
        "$releases/tag/"*) version=${latest##*/} ;;
        *) fail 'Could not resolve the latest release.' ;;
    esac
    version=${version#v}
    case "$version" in
        ''|*[!0-9A-Za-z.+-]*|[!0-9]*) fail 'Invalid release version.' ;;
    esac

    destination=${QRLKIT_INSTALL_DIR:-"$HOME/.local/bin"}
    # Resolve relative destinations before entering the temporary directory.
    case "$destination" in
        /*) ;;
        *) destination=$PWD/$destination ;;
    esac
    temporary=$(mktemp -d)
    trap 'rm -rf "$temporary"' EXIT
    trap 'exit 1' HUP INT TERM
    directory=qrlkit-$version-$os-$arch
    archive=$directory.tar.gz
    url=$releases/download/v$version/$archive
    printf 'Downloading qrlkit %s for %s-%s...\n' "$version" "$os" "$arch"
    curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
        --output "$temporary/$archive" "$url"
    curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
        --output "$temporary/$archive.sha256" "$url.sha256"
    (
        cd "$temporary"
        if [ "$checksum" = sha256sum ]; then
            sha256sum -c "$archive.sha256"
        else
            shasum -a 256 -c "$archive.sha256"
        fi
        tar -xzf "$archive" "$directory/qrlkit"
    )
    [ -f "$temporary/$directory/qrlkit" ] && [ ! -L "$temporary/$directory/qrlkit" ] \
        || fail 'Release archive does not contain a regular qrlkit binary.'
    mkdir -p "$destination"
    # Stage beside the destination so upgrades replace the binary atomically.
    staged=$(mktemp "$destination/.qrlkit.XXXXXX")
    trap 'rm -rf "$temporary"; rm -f "$staged"' EXIT
    cp "$temporary/$directory/qrlkit" "$staged"
    chmod 755 "$staged"
    [ ! -d "$destination/qrlkit" ] || fail 'The destination qrlkit is a directory.'
    mv -f "$staged" "$destination/qrlkit"
    printf 'Installed qrlkit %s to %s/qrlkit\n' "$version" "$destination"
    setup_path
}

# Keep execution at the end so piping the script to sh reads the definition first.
main "$@"
