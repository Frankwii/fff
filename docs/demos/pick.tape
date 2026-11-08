# fff pick in a command substitution, and choosing among piped paths.
Output docs/demos/pick.gif
Require fff
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 16
Set Width 1280
Set Height 780
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 90ms

Hide
Type "exec docs/demos/session.sh"
Enter
Sleep 1.5s
Show

Type 'nvim "$(fff pick)"' Sleep 500ms Enter Sleep 1.5s
Type@200ms "todo" Sleep 1.2s Enter Sleep 2s
Type ":q" Sleep 300ms Enter Sleep 800ms
Type "grep -rl TODO . | fff | xargs -o -r nvim" Sleep 500ms Enter Sleep 1.5s
Type@200ms "sob" Sleep 1.2s Enter Sleep 2s
Type ":q" Sleep 300ms Enter Sleep 1s
