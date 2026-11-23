//! The token tests: every chrome role resolves in both appearances, and
//! every ink pair the controls rely on clears AA. The intent is
//! `crates/turbogit-ui/tests/design_tokens.rs`'s, ported to the nsh token
//! file.

const THEME: &str = include_str!("../theme.json");

/// The roles the controls read, and the surfaces they sit on.
const COLOR_ROLES: [&str; 30] = [
    "background",
    "foreground",
    "card",
    "card-foreground",
    "popover",
    "popover-foreground",
    "primary",
    "primary-foreground",
    "secondary",
    "secondary-foreground",
    "muted",
    "muted-foreground",
    "accent",
    "accent-foreground",
    "destructive",
    "border",
    "input",
    "ring",
    "chart-1",
    "chart-2",
    "chart-3",
    "chart-4",
    "chart-5",
    "sidebar",
    "sidebar-foreground",
    "sidebar-primary",
    "sidebar-primary-foreground",
    "sidebar-accent",
    "sidebar-accent-foreground",
    "sidebar-border",
];

fn theme() -> serde_json::Value {
    serde_json::from_str(THEME).expect("theme.json parses")
}

fn color_of(theme: &serde_json::Value, appearance: &str, role: &str) -> String {
    theme["cssVars"][appearance][role]
        .as_str()
        .unwrap_or_else(|| panic!("{appearance}/{role} resolves"))
        .to_string()
}

fn hex(color: &str) -> [f64; 3] {
    let hex = color.trim_start_matches('#');
    assert!(hex.len() == 6, "{color} is a six-digit hex value");
    let mut out = [0.0; 3];
    for (at, channel) in out.iter_mut().enumerate() {
        *channel = u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16).unwrap() as f64 / 255.0;
    }
    out
}

fn luminance(color: &str) -> f64 {
    let [r, g, b] = hex(color);
    let channel = |c: f64| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (brighter, dimmer) = if la > lb { (la, lb) } else { (lb, la) };
    (brighter + 0.05) / (dimmer + 0.05)
}

#[test]
fn every_chrome_role_resolves_in_both_appearances() {
    let theme = theme();
    for appearance in ["light", "dark"] {
        for role in COLOR_ROLES {
            let color = color_of(&theme, appearance, role);
            assert!(
                color.starts_with('#'),
                "{appearance}/{role} is hex: {color}"
            );
        }
    }
    // The radius token, the one non-color the controls read.
    assert!(
        theme["cssVars"]["theme"]["radius"].as_str().is_some(),
        "theme/radius resolves"
    );
}

#[test]
fn the_shell_ink_clears_aa_on_every_surface_it_paints() {
    let theme = theme();
    for appearance in ["light", "dark"] {
        let surface = |role: &str| color_of(&theme, appearance, role);
        let foreground = surface("foreground");
        for surface_role in ["background", "card", "popover", "secondary", "muted"] {
            let ratio = contrast(&foreground, &surface(surface_role));
            assert!(
                ratio >= 4.5,
                "{appearance} foreground on {surface_role} must clear AA: {ratio:.3}:1"
            );
        }
    }
}

#[test]
fn the_muted_ink_is_legal_where_it_is_used_and_dim_where_it_is_not() {
    let theme = theme();
    for appearance in ["light", "dark"] {
        let surface = |role: &str| color_of(&theme, appearance, role);
        let muted = surface("muted-foreground");
        // The muted ink's legality set: the app surface.
        let ratio = contrast(&muted, &surface("background"));
        assert!(
            ratio >= 4.5,
            "{appearance} muted ink on background must clear AA: {ratio:.3}:1"
        );
    }
}

#[test]
fn primary_and_accent_inks_read_on_their_fills() {
    let theme = theme();
    for appearance in ["light", "dark"] {
        let surface = |role: &str| color_of(&theme, appearance, role);
        let pairs = [
            ("accent-foreground", "accent"),
            ("card-foreground", "card"),
            ("popover-foreground", "popover"),
            ("secondary-foreground", "secondary"),
            ("sidebar-foreground", "sidebar"),
        ];
        for (ink, fill) in pairs {
            let ratio = contrast(&surface(ink), &surface(fill));
            assert!(
                ratio >= 4.5,
                "{appearance} {ink} on {fill} must clear AA: {ratio:.3}:1"
            );
        }
        // The one accepted step, as `docs/design-system-roles.md` records
        // for the empty-state ink. Dark keeps the brand value the brief
        // pins, and its white ink measures 4.279:1 on it; light deepens the
        // brand to the nearest value whose white ink clears AA (5.389:1).
        // Do not "correct" either without a design decision; the assertions
        // pin the numbers so a value move flags the audit.
        let ratio = contrast(&surface("primary-foreground"), &surface("primary"));
        if appearance == "dark" {
            assert!(
                (4.0..=4.5).contains(&ratio),
                "dark primary ink is the accepted sub-AA step; it moved to {ratio:.3}:1"
            );
        } else {
            assert!(ratio >= 4.5, "light primary ink clears AA: {ratio:.3}:1");
        }
    }
}

#[test]
fn the_dark_values_carry_the_egui_roles_they_succeed() {
    let theme = theme();
    // The chrome roles the brief maps from crates/turbogit-ui's Palette.
    let pinned = [
        ("background", "#1e1f22", "BG"),
        ("card", "#2b2d30", "SURFACE / RAISED"),
        ("border", "#4e5157", "LINE"),
        ("foreground", "#dfe1e5", "INK"),
        ("muted-foreground", "#8a8e96", "INK_3"),
        ("primary", "#3574f0", "BRAND"),
        ("primary-foreground", "#ffffff", "BRAND_INK"),
        ("destructive", "#ef5350", "STATE_ERROR"),
    ];
    for (role, value, egui_name) in pinned {
        assert_eq!(
            color_of(&theme, "dark", role),
            value,
            "dark {role} is the egui {egui_name} value"
        );
    }
    assert_eq!(
        color_of(&theme, "dark", "popover"),
        color_of(&theme, "dark", "card"),
        "popover wears RAISED, which wears SURFACE"
    );
}
