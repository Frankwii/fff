# Setup and integration

fff works on its own, but a few things outside it make it much nicer: your
shell following fff when you leave, completions, man pages, and on Linux,
fff as your desktop's file manager and file chooser. Each is one
`fff setup` command.

| What | Command |
| --- | --- |
| A first configuration and all of the below, from a few questions | `fff setup` |
| [Change directory on exit](#change-directory-on-exit) | `fff setup --init SHELL` |
| [Shell completions](#shell-completions) | `fff setup --completions SHELL` |
| [Man pages](#man-pages) | `fff setup --man` |
| [Open folders in fff](#the-desktops-file-manager) (Linux) | `fff setup --desktop` |
| [Firefox's file dialogs](#firefox-and-other-applications-file-dialogs) (Linux) | `fff setup --browser` |
| Check all of the above | `fff setup --check-config` |

`fff setup` on its own asks about each of these that isn't set up yet,
shows what it would write, and writes it if you say yes: it adds the lines
for your shell to its startup file, under `# Added by fff setup`.

`fff setup --check-config` checks your configuration and keys, which
optional programs you have, and whether the desktop integration is
working, and says what to fix.

## Change directory on exit

A program can't change the directory of the shell that started it, so when
you leave fff with `!`, `g!` or Enter on a file, a small shell function
does it. Add one line to your shell's startup file:

```sh
eval "$(fff setup --init zsh)"     # ~/.zshrc
eval "$(fff setup --init bash)"    # ~/.bashrc
fff setup --init fish | source     # ~/.config/fish/config.fish
```

This defines a function named `fff` that runs the real program, then
changes to the folder it chose. Everything else passes through unchanged:
`fff pick` still prints paths, and options work as before.

Without the function, `fff` prints the folder to change to, so
`cd "$(command fff)"` works too. The first time it prints the folder to
your terminal, where nothing reads it, it says which line to add.

## Shell completions

Completions cover fff's commands and options, and their values: folders,
palette names (your own too), workspace names and keys.

**From the Arch package**, they're installed for bash, zsh and fish
already: there's nothing to do.

**The simplest way** is to load them from your shell's startup file:

```sh
source <(fff setup --completions bash)   # ~/.bashrc
source <(fff setup --completions zsh)    # ~/.zshrc, after compinit
fff setup --completions fish | source    # ~/.config/fish/config.fish
```

**Or save the script once**, where your shell looks for completions. It
only calls `fff` back, so it doesn't need updating when you upgrade fff.

- **bash**, with the bash-completion package:
  ```sh
  mkdir -p ~/.local/share/bash-completion/completions
  fff setup --completions bash > ~/.local/share/bash-completion/completions/fff
  ```
- **fish**:
  ```sh
  fff setup --completions fish > ~/.config/fish/completions/fff.fish
  ```
- **zsh**: save it as `_fff` in a folder that is in `$fpath` *when compinit
  runs*. For example, in `~/.zshrc`:
  ```sh
  fpath=(~/.config/zsh/completions $fpath)
  autoload -Uz compinit && compinit
  ```
  then:
  ```sh
  mkdir -p ~/.config/zsh/completions
  fff setup --completions zsh > ~/.config/zsh/completions/_fff
  ```
  With a framework such as oh-my-zsh, or a plugin manager, compinit runs
  inside it: add the folder to `$fpath` before the framework loads, or use
  the `source <(…)` line after it instead.

**If zsh doesn't pick it up**, open a new shell and run `print
$_comps[fff]`: it should print `_fff`. If it doesn't, the folder wasn't in
`$fpath` when compinit ran. compinit also caches what it found in
`~/.zcompdump`; deleting that file makes the next shell look again.

Elvish and PowerShell are supported as well (`fff setup --completions
elvish`, `powershell`).

## Man pages

```sh
fff setup --man
```

writes `man fff` and a page per command (`man fff-pick`, `man fff-seek`…)
into your own man folder, usually `~/.local/share/man`. If it can't find
one, it says so; give it a folder instead: `fff setup --man
~/.local/share/man/man1`. Run it again after upgrading. The Arch package
installs the pages already.

## The desktop's file manager

*Linux only.*

```sh
fff setup --desktop
```

makes fff open the folders other programs open: `xdg-open ~/Downloads`, a
launcher's "Files", a download's "Show in folder". Each opens fff in a new
terminal window (`fff window`), with the file highlighted when there is
one.

In a window of its own there's no shell to return to, so a few keys act
like a graphical file manager's: Enter goes into folders, opening a file
closes the window, `q` closes it, and `!` turns the window into a shell in
the folder you're in.

Try it:

```sh
xdg-mime query default inode/directory   # should say fff.desktop
xdg-open ~/Downloads                      # opens fff in a new window
```

Setup tells you which program opened folders before, and how to switch
back. It writes:

- a desktop entry, `~/.local/share/applications/fff.desktop`, made the
  default for folders;
- a small launcher script it runs, in `~/.local/share/fff`;
- a D-Bus service so that "Show in folder" highlights the file (when no
  other file manager is running).

Run it again after moving the fff binary or changing your terminal. Files
setup wrote are replaced; it never overwrites anything else.

**"Show in folder" doesn't open fff.** The D-Bus service starts fff with
the bus's environment, which must know your display. Desktop environments
arrange this; with a bare window manager started by `startx`, add this to
your startup (`~/.xinitrc`, or your window manager's autostart):

```sh
dbus-update-activation-environment --systemd DISPLAY XAUTHORITY
```

(`WAYLAND_DISPLAY` instead, under Wayland.)

### Floating windows

A folder opened from another program is best shown as a floating window
over what you're doing. fff gives these windows the class `fff-picker` (in
terminals that support one), so your window manager can float them. In i3
(`~/.config/i3/config`):

```
for_window [class="fff-picker"] floating enable, resize set 1500 1000, move position center
```

In sway: `for_window [app_id="fff-picker"] floating enable`. In dwm, a
`rules[]` entry for the class `fff-picker` with `isfloating` set.

To pick the terminal and the class yourself, set `fff_terminal` in the
configuration, then run `fff setup --desktop` (and `--browser`) again:

```toml
[open]
fff_terminal = "alacritty --class fff-picker -e"
```

Other terminals: `st -c fff-picker -e`, `kitty --class fff-picker`,
`foot --app-id fff-picker`, `xterm -class fff-picker -e`. The terminal must
stay open until fff ends; one that hands the command to a running server
and returns at once doesn't work here.

## Firefox and other applications' file dialogs

*Linux only.*

Firefox, and other applications that use the desktop portal, can choose
files with fff: uploading a file, saving a download, choosing a folder.

1. Install
   [xdg-desktop-portal-termfilechooser](https://github.com/hunkyburrito/xdg-desktop-portal-termfilechooser).
   On Arch, it's the AUR package
   `xdg-desktop-portal-termfilechooser-hunkyburrito-git`.
2. Set it up and restart the portal:
   ```sh
   fff setup --browser
   systemctl --user restart xdg-desktop-portal
   ```
3. In Firefox, open `about:config`, set
   `widget.use-xdg-desktop-portal.file-picker` to `1`, and restart
   Firefox.

File dialogs then open fff in a floating window (see [above](#floating-windows)).

### Using the dialog

| Dialog | How to choose |
| --- | --- |
| Open a file | Enter on the file |
| Open several files | Space to mark them, then Enter (or Ctrl+Enter) |
| Choose a folder | Go into it, then `!` or Ctrl+s |
| Save | Go to the folder, then Ctrl+s (or Escape, then `!`) |
| Save under another name | Go to the folder, type the name, then Enter |

The empty query line says how to choose, and in a save dialog the query
box's title shows the name the file will get. `q` (or Ctrl+c while typing)
cancels. Enter on a folder goes into it, except when saving, where Enter
saves under the typed or suggested name. Saving over an existing
file always asks first, and `k` there keeps both (`paper (1).pdf`).

Everything else works as usual, contents search and file operations
included, so you can make the folder to save into with `n`.

### If dialogs don't open fff

Check the portal on its own:

```sh
gdbus call --session --dest org.freedesktop.portal.Desktop \
  --object-path /org/freedesktop/portal/desktop \
  --method org.freedesktop.portal.FileChooser.OpenFile "" "Test" "{}"
```

If no fff window opens, the portal is the problem: `journalctl --user -u
xdg-desktop-portal` says which file chooser it picked. If one opens, the
application isn't using the portal.

**Qt applications** (Telegram, for one) only use the portal when told to.
Set this where your session's environment is set (`~/.xinitrc`,
`~/.profile`), before they start:

```sh
export QT_QPA_PLATFORMTHEME=xdgdesktopportal
```

**GTK applications** other than Firefox (GIMP, Inkscape) need
`GTK_USE_PORTAL=1` the same way.

`fff setup --check-config` warns when Qt applications would draw their own
dialog.

### Answering dialogs from scripts

The dialogs run `fff explorer`, which you can use yourself wherever a
script needs a file, a folder or a place to save:

```sh
fff explorer open ~/Downloads             # choose one file
fff explorer open-multiple                # choose several
fff explorer folder                       # choose a folder
fff explorer save ~/Downloads/paper.pdf   # choose where to save paper.pdf
```

It prints the answer, one absolute path per line, and exits with 130 when
cancelled.

## On macOS

The shell function, completions and man pages work the same. The desktop
integration and file dialogs need a Linux desktop: on macOS,
`fff setup --desktop` and `--browser` say so and change nothing.
Folders also don't update live on macOS; they refresh on a timer instead.
