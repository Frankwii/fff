# Archiving a selection: fff pick chooses this year's exams from four
# folders, and tar keeps their folders, relative to where fff started.
Output docs/demos/task-archive.gif
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

Type "cd teaching" Sleep 300ms Enter Sleep 500ms
Type@60ms "fff pick --relative -0 | xargs -0 -r tar czf exams.tar.gz" Sleep 500ms Enter Sleep 1.5s
Type@150ms "2025-2026exam" Sleep 1.5s
Escape Sleep 300ms Escape Sleep 300ms
Type "m" Sleep 1.5s
Enter Sleep 1s
Type "tar tzf exams.tar.gz" Sleep 300ms Enter Sleep 3s
