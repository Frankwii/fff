# Finding an exam question again: the contents query finds the text, the
# names query keeps only the exams.
Output docs/demos/task-contents.gif
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

Type "cd teaching && fff -d 4" Sleep 400ms Enter Sleep 1.5s
Type "i" Sleep 300ms Tab Sleep 500ms
Type@150ms "schwarz" Sleep 2.5s
Tab Sleep 500ms
Type@150ms "exam" Sleep 2.5s
Escape Sleep 300ms Type "j" Sleep 1.2s
Type "j" Sleep 1.5s Type "j" Sleep 1.5s
Type "q" Sleep 800ms
