# gx and gX with zsh (src/zsh.rs): `zsh -i -c <this> zsh PATH...`, after the
# user's configuration. The line is edited with zsh's own line editor, the
# paths already after the cursor, and run as zsh runs any line. $FFF_PROMPT
# is its prompt. For gx, which comes back to fff, $FFF_LINE is where the
# line that ran is written; for gX it is unset, and fff's screen is gone.
typeset -g __fff_line=${${(j: :)${(q-)@}}:+ ${(j: :)${(q-)@}}} __fff_started=
set --
# A prompt has the history file read; `zsh -c` only writes it.
[[ -r $HISTFILE ]] && fc -R
__fff_start() { [[ -n $__fff_started ]] || { __fff_started=1; CURSOR=0 } }
autoload -Uz add-zle-hook-widget && add-zle-hook-widget line-init __fff_start
# Tab completes the line as at a prompt, not as a variable's value.
typeset -g compcontext='values:value:{_normal}'
vared -h -p "$FFF_PROMPT" __fff_line || exit 0
unset compcontext
add-zle-hook-widget -d line-init __fff_start
unfunction __fff_start
[[ $__fff_line == [[:space:]]# ]] && exit 0
print -rs -- $__fff_line
if [[ -n $FFF_LINE ]]; then
  print -rn -- $__fff_line >| $FFF_LINE
  # The command runs on the terminal's own screen, out of fff's.
  print -n '\e[?1049l'
fi
# Outside a prompt vared clears its line when done; it stays above what
# the command prints, as in a terminal.
print -nP -- $FFF_PROMPT
print -r -- $__fff_line
unset FFF_LINE FFF_PROMPT
eval -- $__fff_line
