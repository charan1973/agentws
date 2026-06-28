use anyhow::{anyhow, Result};

/// Print the `activate`/`deactivate` shell integration for `eval "$(agentws init-shell)"`.
///
/// `activate`/`deactivate` must live in the shell (they change the shell's cwd and
/// environment, which a child process cannot do), so we emit a shell *function*
/// named `agentws` that intercepts those two subcommands and delegates everything
/// else to the real binary via `command agentws`.
pub fn run(shell: Option<String>) -> Result<()> {
    let shell = shell.unwrap_or_else(detect_shell);
    match shell.as_str() {
        "bash" | "zsh" => print_posix(&shell),
        "fish" => print_fish(),
        other => Err(anyhow!(
            "unsupported shell '{other}'. Try: bash, zsh, or fish (or set $SHELL)."
        )),
    }
}

fn detect_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| {
            std::path::Path::new(&s)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "bash".to_string())
}

fn print_posix(shell: &str) -> Result<()> {
    let func = r##"agentws() {
  case "$1" in
    activate)
      shift
      if [ $# -lt 1 ]; then
        echo "usage: agentws activate <story> [command...]" >&2
        echo "  cd into the workspace and set \$AGENTWS_WORKSPACE." >&2
        echo "  with a command, run it in the workspace and return here." >&2
        return 2
      fi
      local __aw_story="$1"; shift
      local __aw_root
      __aw_root="$(command agentws open "$__aw_story" 2>/dev/null)" || {
        echo "agentws: no workspace named '$__aw_story'" >&2
        return 1
      }
      _AGENTWS_OLD_PWD="${PWD}"
      cd "$__aw_root" || return $?
      export AGENTWS_WORKSPACE="$__aw_story"
      if [ $# -gt 0 ]; then
        "$@"
        local __aw_rc=$?
        cd "${_AGENTWS_OLD_PWD}" 2>/dev/null
        unset AGENTWS_WORKSPACE _AGENTWS_OLD_PWD
        return $__aw_rc
      fi
      ;;
    deactivate)
      if [ -z "${AGENTWS_WORKSPACE:-}" ]; then
        echo "agentws: no active workspace" >&2
        return 1
      fi
      cd "${_AGENTWS_OLD_PWD:-$HOME}" 2>/dev/null
      unset AGENTWS_WORKSPACE _AGENTWS_OLD_PWD
      ;;
    *)
      command agentws "$@"
      ;;
  esac
}"##;

    let completion = if shell == "zsh" {
        r##"_agentws() {
  local -a subs stories
  subs=(activate deactivate new list use status open delete add remove
        request pending approve deny archive restore mcp mcp-config
        completions init-shell)
  if (( CURRENT == 2 )); then
    _wanted commands expl 'command' compadd -- $subs
  elif [[ $words[2] == activate && $CURRENT == 3 ]]; then
    stories=(${(f)"$(command agentws _list-stories 2>/dev/null)"})
    _wanted stories expl 'story' compadd -- $stories
  fi
}
compdef _agentws agentws"##
    } else {
        r##"_agentws_complete() {
  local cur="${COMP_WORDS[COMP_CWORD]}"
  if [ "${COMP_WORDS[1]}" = "activate" ] && [ "$COMP_CWORD" -eq 2 ]; then
    COMPREPLY=( $(compgen -W "$(command agentws _list-stories 2>/dev/null)" -- "$cur") )
    return
  fi
}
complete -F _agentws_complete agentws"##
    };

    println!("# agentws shell integration for {shell}.");
    println!("# Add to your ~/{shell}.rc:  eval \"$(agentws init-shell {shell})\"");
    println!("{func}");
    println!();
    println!("{completion}");
    Ok(())
}

fn print_fish() -> Result<()> {
    let func = r##"function agentws
    switch $argv[1]
        case activate
            set -e argv[1]
            if test (count $argv) -lt 1
                echo "usage: agentws activate <story> [command...]" >&2
                return 2
            end
            set -l story $argv[1]
            set -e argv[1]
            set -l root (command agentws open $story 2>/dev/null)
            if test -z "$root"
                echo "agentws: no workspace named '$story'" >&2
                return 1
            end
            set -g _AGENTWS_OLD_PWD $PWD
            cd $root; or return $status
            set -gx AGENTWS_WORKSPACE $story
            if test (count $argv) -gt 0
                $argv
                set -l rc $status
                cd $_AGENTWS_OLD_PWD 2>/dev/null
                set -e AGENTWS_WORKSPACE _AGENTWS_OLD_PWD
                return $rc
            end
        case deactivate
            if not set -q AGENTWS_WORKSPACE
                echo "agentws: no active workspace" >&2
                return 1
            end
            cd $_AGENTWS_OLD_PWD 2>/dev/null
            set -e AGENTWS_WORKSPACE _AGENTWS_OLD_PWD
        case '*'
            command agentws $argv
    end
end

complete -c agentws -n '__fish_use_subcommand' -a '(command agentws _list-stories 2>/dev/null)' -d 'workspace'"##;
    println!("# agentws shell integration for fish.");
    println!("# Add to ~/.config/fish/config.fish:  agentws init-shell fish | source");
    println!("{func}");
    Ok(())
}
