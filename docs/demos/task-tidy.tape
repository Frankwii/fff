# Tidying Downloads: every PDF marked at once and moved into the papers
# folder.
Output docs/demos/task-tidy.gif
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

Type "cd Downloads && fff" Sleep 400ms Enter Sleep 1.5s
Type "i" Sleep 300ms Type@150ms ".pdf" Sleep 1s
Escape Sleep 300ms Escape Sleep 300ms
Type "m" Sleep 1.5s
Type "/" Sleep 600ms Type@100ms "~/papers" Sleep 800ms Enter Sleep 1.5s
Type "v" Sleep 2.5s
Type "q" Sleep 800ms
