# Freeing space: sorted by size, files only, a few levels deep, the largest
# files come first wherever they are.
Output docs/demos/task-space.gif
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

Type "fff" Sleep 400ms Enter Sleep 1.5s
Type "gs" Sleep 1s Type@150ms "size" Sleep 600ms Enter Sleep 2s
Type "+" Sleep 500ms Type "+" Sleep 500ms Type "+" Sleep 1s
Type "gd" Sleep 2s
Type "j" Sleep 1s
Type "x" Sleep 2.5s
Type "q" Sleep 800ms
