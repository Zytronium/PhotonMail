pub type Rgb = (u8, u8, u8);

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
    selected: (55, 50, 95),
    hover: (40, 40, 54),
    close_hover: (150, 35, 35),
    text: (235, 235, 245),
    muted: (150, 150, 170),
    accent: (109, 74, 255),
    divider: (45, 45, 58),
    danger: (220, 70, 90),
    success: (80, 200, 120),
};

/// Background of a secondary button while hovered (the app's `Action` / `Chip` hover colour).
pub const HOVER_STRONG: Rgb = (50, 50, 68);
