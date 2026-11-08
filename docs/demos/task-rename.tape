# Renaming a folder of photos with one Neovim substitution.
Output docs/demos/task-rename.gif
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

Type "cd Pictures/mallorca-2026 && fff" Sleep 400ms Enter Sleep 1.5s
Type "r" Sleep 1.5s
Type@80ms ":%s/IMG_/mallorca-/" Sleep 800ms Enter Sleep 1.5s
Type "ZZ" Sleep 2.5s
Type "q" Sleep 800ms
