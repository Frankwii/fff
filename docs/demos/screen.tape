# The still of the main screen that the tutorial labels.
Output target/demo-screen.gif
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
Type "fff" Sleep 400ms Enter Sleep 1.5s
Type "j" Sleep 1.5s
Screenshot docs/demos/screen.png
Sleep 500ms
Type "q" Sleep 500ms
