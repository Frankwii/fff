# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- Content-based search, similar to grep.
- Tab completion of program options in the gx/gX prompt (for example
  `dragon --and-exit`), which needs per-program knowledge.
- Launching a program in the background while browsing continues, for GUI
  programs that should not pause fff.

Behavior, keybindings and persistence for these features remain to be designed
when the work is requested.

File operations shipped in 0.3.0; running programs (gx/gX, E), the shell
integration and remembered locations in 0.4.0. See [the operation
guide](OPERATIONS.md). Buffer
editors share write-on-close application and unwritten-edit discard behavior.

The current release is 0.4.0. Version 1.0 is planned once the remaining roadmap
above is implemented. Earlier versions will not be retroactively renumbered.
