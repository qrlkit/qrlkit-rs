use clap::ValueEnum;

#[derive(Clone, Debug, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

pub fn init(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash | Shell::Zsh => {
            r#"qrlkit() {
    local qrl_file qrl_target qrl_status
    qrl_file=$(mktemp) || return
    if QRL_CD_FILE="$qrl_file" command qrlkit "$@"; then
        qrl_status=0
    else
        qrl_status=$?
    fi
    if [ -s "$qrl_file" ]; then
        IFS= read -r qrl_target < "$qrl_file"
        if builtin cd -- "$qrl_target"; then
            :
        else
            qrl_status=$?
        fi
    fi
    command rm -f -- "$qrl_file"
    return "$qrl_status"
}
"#
        }
        Shell::Fish => {
            r#"function qrlkit
    set -l qrl_file (mktemp)
    or return
    set -lx QRL_CD_FILE $qrl_file
    command qrlkit $argv
    set -l qrl_status $status
    if test -s "$qrl_file"
        read -l qrl_target < "$qrl_file"
        builtin cd -- "$qrl_target"
        or set qrl_status $status
    end
    command rm -f -- "$qrl_file"
    return $qrl_status
end
"#
        }
    }
}
