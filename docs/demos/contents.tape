# Searching what files contain, PDFs included, one row per match.
Output docs/demos/contents.gif
Require fff
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 16
Set Width 1280
Set Height 720
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 90ms

Hide
Type "source docs/demos/env.sh && clear"
Enter
Show

Type "fff --depth 3" Sleep 400ms Enter Sleep 1.5s
Type "i" Sleep 300ms Tab Sleep 600ms
Type@120ms "Sobolev embedding" Sleep 2s
Escape Sleep 300ms Type "j" Sleep 1.2s
Type "j" Sleep 1.2s Type "j" Sleep 1.5s
Tab Sleep 300ms Type "i" Sleep 300ms Type@200ms "pdf" Sleep 2s
Escape Sleep 300ms Escape Sleep 300ms
Type "q" Sleep 800ms
