# Working on picker

Read [docs/SPECIFICATION.md](docs/SPECIFICATION.md) before changing program behavior.
It consolidates the product intent, active acceptance examples, proposed technical
direction, and unresolved decisions.

Read `tui_functionality_examples/AGENTS.md` and relevant active examples as well.
Explicit user instructions take precedence. Active examples take precedence over
the consolidated specification and `KEYBINDINGS.md` if they disagree. Do not treat
examples marked `STATUS: Delayed until future versions` as active requirements,
even when an older conversation or reference script mentions that functionality.

Keep behavior changes traceable to an active example or a user decision. Preserve
unresolved decisions as unresolved until clarified; do not change curated examples
to make an implementation pass. Technical proposals are not settled product rules.

The current work is specification and initialization only. Do not infer permission
to implement the application merely from the presence of this specification.
