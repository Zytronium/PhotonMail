pub type Rgb = (u8, u8, u8);

pub const ACCOUNT_COLORS: [Rgb; 17] = [
    (240, 40, 40),
    (230, 100, 60),
    (240, 140, 40),
    (230, 180, 60),
    (160, 200, 70),
    (100, 190, 75),
    (45, 180, 90),
    (40, 190, 170),
    (0, 150, 136),
    (40, 180, 220),
    (80, 160, 235),
    (60, 110, 220),
    (109, 74, 255),
    (150, 90, 230),
    (210, 80, 220),
    (235, 90, 150),
    (220, 70, 90),
];

pub struct Theme {
    pub bg: Rgb,
    pub panel: Rgb,
    pub titlebar: Rgb,
    pub selected: Rgb,
    pub hover: Rgb,
    pub close_hover: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub divider: Rgb,
    pub danger: Rgb,
    pub success: Rgb,
}

pub const DARK: Theme = Theme {
    bg: (22, 22, 29),
    panel: (30, 30, 40),
    titlebar: (18, 18, 24),
    selected: (47, 44, 82),
    hover: (40, 40, 54),
    close_hover: (150, 35, 35),
    text: (235, 235, 245),
    muted: (150, 150, 170),
    accent: (109, 74, 255),
    divider: (42, 42, 55),
    danger: (220, 70, 90),
    success: (80, 200, 120),
};

/// Background of a secondary button while hovered (the app's `Action` / `Chip` hover colour).
pub const HOVER_STRONG: Rgb = (50, 50, 68);

/// Accent colour while hovered (primary buttons).
pub const ACCENT_HOVER: Rgb = (124, 96, 255);

/// Fill for count badges that sit on a neutral row.
pub const BADGE: Rgb = (52, 52, 70);
