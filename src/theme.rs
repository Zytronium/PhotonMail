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

// Freagle Mail theme — red, white, and blue patriotic colors
pub const DARK: Theme = Theme {
    bg: (20, 20, 20),          // Darker neutral gray background (no tint over flag)
    panel: (25, 25, 25),       // Darker neutral gray panel
    titlebar: (18, 18, 18),    // Darker gray titlebar
    selected: (35, 35, 35),    // Darker gray selection
    hover: (30, 30, 30),       // Darker gray hover
    close_hover: (180, 30, 30), // Bright red close
    text: (240, 240, 240),     // Near-white
    muted: (160, 160, 160),    // Gray muted
    accent: (40, 80, 200),     // Bold patriotic blue (buttons only)
    divider: (30, 30, 30),     // Darker gray divider
    danger: (220, 50, 50),     // Bright red danger
    success: (40, 180, 100),   // Green (kept for success states)
};

/// Background of a secondary button while hovered (the app's `Action` / `Chip` hover colour).
pub const HOVER_STRONG: Rgb = (40, 40, 90); // Blue-tinted strong hover
