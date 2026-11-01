//! Semantic mappings of upstream palette colors, verified 2026-11-01.
//! Sources and role mappings: docs/PALETTES.md. No network access at runtime.
pub const NAMES: &[&str] = &[
    "midnight",
    "light",
    "forest",
    "catppuccin-latte",
    "catppuccin-frappe",
    "catppuccin-macchiato",
    "catppuccin-mocha",
    "nord",
    "gruvbox-dark",
    "gruvbox-light",
    "tokyonight-night",
    "tokyonight-storm",
    "tokyonight-moon",
    "tokyonight-day",
];
pub fn colors(name: &str) -> Option<[u32; 12]> {
    Some(match name {
        "catppuccin-latte" => [
            0xeff1f5, 0xe6e9ef, 0x4c4f69, 0x6c6f85, 0x9ca0b0, 0x1e66f5, 0xdf8e1d, 0x40a02b,
            0xd20f39, 0xccd0da, 0x1e66f5, 0xeff1f5,
        ],
        "catppuccin-frappe" => [
            0x303446, 0x292c3c, 0xc6d0f5, 0xa5adce, 0x737994, 0x8caaee, 0xe5c890, 0xa6d189,
            0xe78284, 0x414559, 0x8caaee, 0x303446,
        ],
        "catppuccin-macchiato" => [
            0x24273a, 0x1e2030, 0xcad3f5, 0xa5adcb, 0x6e738d, 0x8aadf4, 0xeed49f, 0xa6da95,
            0xed8796, 0x363a4f, 0x8aadf4, 0x24273a,
        ],
        "catppuccin-mocha" => [
            0x1e1e2e, 0x181825, 0xcdd6f4, 0xa6adc8, 0x6c7086, 0x89b4fa, 0xf9e2af, 0xa6e3a1,
            0xf38ba8, 0x313244, 0x89b4fa, 0x1e1e2e,
        ],
        "nord" => [
            0x2e3440, 0x3b4252, 0xd8dee9, 0x8fbcbb, 0x4c566a, 0x88c0d0, 0xebcb8b, 0xa3be8c,
            0xbf616a, 0x434c5e, 0x5e81ac, 0xeceff4,
        ],
        "gruvbox-dark" | "gruvbox" => [
            0x282828, 0x282828, 0xebdbb2, 0x928374, 0x665c54, 0xfe8019, 0xfabd2f, 0xb8bb26,
            0xfb4934, 0x504945, 0xfe8019, 0x282828,
        ],
        "gruvbox-light" => [
            0xfbf1c7, 0xfbf1c7, 0x3c3836, 0x928374, 0xbdae93, 0xaf3a03, 0xb57614, 0x79740e,
            0x9d0006, 0xebdbb2, 0xaf3a03, 0xfbf1c7,
        ],
        "tokyonight-night" | "tokyonight" => [
            0x1a1b26, 0x15161e, 0xc0caf5, 0xa9b1d6, 0x414868, 0x7dcfff, 0xe0af68, 0x9ece6a,
            0xf7768e, 0x414868, 0x7aa2f7, 0x1a1b26,
        ],
        "tokyonight-storm" => [
            0x24283b, 0x1d202f, 0xc0caf5, 0xa9b1d6, 0x414868, 0x7dcfff, 0xe0af68, 0x9ece6a,
            0xf7768e, 0x414868, 0x7aa2f7, 0x24283b,
        ],
        "tokyonight-moon" => [
            0x222436, 0x1b1d2b, 0xc8d3f5, 0x828bb8, 0x444a73, 0x86e1fc, 0xffc777, 0xc3e88d,
            0xff757f, 0x444a73, 0x82aaff, 0x222436,
        ],
        "tokyonight-day" => [
            0xe1e2e7, 0xe1e2e7, 0x3760bf, 0x6172b0, 0xa1a6c5, 0x007197, 0x8c6c3e, 0x587539,
            0xf52a65, 0xb4b5b9, 0x2e7de9, 0xe1e2e7,
        ],
        _ => return None,
    })
}
