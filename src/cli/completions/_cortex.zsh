#compdef cortex
# cortex zsh completion — delegates to `cortex __complete`, which derives
# candidates from the single ACTION_SPECS registry (+ live DB values).
_cortex() {
  local -a candidates
  local cur=${words[CURRENT]}
  local prev=${words[CURRENT-1]}

  # First positional: complete action/command names.
  if (( CURRENT == 2 )); then
    candidates=("${(@f)$(cortex __complete actions 2>/dev/null)}")
    _describe -t actions 'cortex command' candidates
    return
  fi

  local action=${words[2]}

  # After a flag that takes a value, complete the value (live hosts/apps/etc.).
  if [[ $prev == --* || $prev == -[a-z] ]]; then
    local -a vals
    vals=("${(@f)$(cortex __complete value $prev 2>/dev/null)}")
    if (( ${#vals} )); then
      compadd -- ${vals%%$'\t'*}
      return
    fi
  fi

  # Otherwise complete this command's flags.
  candidates=("${(@f)$(cortex __complete flags $action 2>/dev/null)}")
  _describe -t flags 'flag' candidates
}
_cortex "$@"
