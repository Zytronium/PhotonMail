use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
    Black,
}

impl ThemeMode {
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::Light, "Light"),
        (Self::Dark, "Dark"),
        (Self::Black, "Black"),
    ];

    const fn index(self) -> usize {
        match self {
            Self::Light => 0,
            Self::Dark => 1,
            Self::Black => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentColor {
    Red,
    Yellow,
    Green,
    Cyan,
    Blue,
    #[default]
    Purple,
    Pink,
    Gray,
}

impl AccentColor {
    pub const ALL: [(Self, &'static str, Rgb); 8] = [
        (Self::Red, "Red", (190, 48, 62)),
        (Self::Yellow, "Yellow", (150, 108, 0)),
        (Self::Green, "Green", (35, 130, 72)),
        (Self::Cyan, "Cyan", (0, 115, 140)),
        (Self::Blue, "Blue", (45, 95, 195)),
        (Self::Purple, "Purple", (109, 74, 255)),
        (Self::Pink, "Pink", (190, 45, 120)),
        (Self::Gray, "Gray", (100, 105, 120)),
    ];

    const fn index(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Yellow => 1,
            Self::Green => 2,
            Self::Cyan => 3,
            Self::Blue => 4,
            Self::Purple => 5,
            Self::Pink => 6,
            Self::Gray => 7,
        }
    }

    pub const fn rgb(self) -> Rgb {
        Self::ALL[self.index()].2
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundHue {
    Red,
    Yellow,
    Green,
    Cyan,
    Blue,
    #[default]
    Purple,
    Pink,
    Gray,
}

impl BackgroundHue {
    pub const ALL: [(Self, &'static str, Rgb); 8] = [
        (Self::Red, "Red", (255, 74, 74)),
        (Self::Yellow, "Yellow", (255, 255, 74)),
        (Self::Green, "Green", (74, 255, 74)),
        (Self::Cyan, "Cyan", (74, 255, 255)),
        (Self::Blue, "Blue", (74, 164, 255)),
        (Self::Purple, "Purple", (109, 74, 255)),
        (Self::Pink, "Pink", (255, 74, 164)),
        (Self::Gray, "Gray", (164, 164, 164)),
    ];

    const fn index(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Yellow => 1,
            Self::Green => 2,
            Self::Cyan => 3,
            Self::Blue => 4,
            Self::Purple => 5,
            Self::Pink => 6,
            Self::Gray => 7,
        }
    }

    pub const fn rgb(self) -> Rgb {
        Self::ALL[self.index()].2
    }

    const fn degrees(self) -> u16 {
        match self {
            Self::Red => 0,
            Self::Yellow => 60,
            Self::Green => 120,
            Self::Cyan => 180,
            Self::Blue => 210,
            Self::Purple => 251,
            Self::Pink => 330,
            Self::Gray => 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Rgb,
    pub panel: Rgb,
    pub titlebar: Rgb,
    pub selected: Rgb,
    pub hover: Rgb,
    pub hover_strong: Rgb,
    pub close_hover: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub accent_text: Rgb,
    pub accent_hover: Rgb,
    pub divider: Rgb,
    pub badge: Rgb,
    pub danger: Rgb,
    pub danger_bg: Rgb,
    pub warning: Rgb,
    pub warning_bg: Rgb,
    pub success: Rgb,
}

const fn mix(from: Rgb, to: Rgb, amount: u16) -> Rgb {
    (
        blend_channel(from.0, to.0, amount),
        blend_channel(from.1, to.1, amount),
        blend_channel(from.2, to.2, amount),
    )
}

const fn blend_channel(from: u8, to: u8, amount: u16) -> u8 {
    ((from as u16 * (100 - amount) + to as u16 * amount) / 100) as u8
}

const fn set_hue_and_saturation(color: Rgb, hue: u16, saturation: u8) -> Rgb {
    let red = color.0 as u16;
    let green = color.1 as u16;
    let blue = color.2 as u16;
    let max = if red > green {
        if red > blue { red } else { blue }
    } else if green > blue {
        green
    } else {
        blue
    };
    let min = if red < green {
        if red < blue { red } else { blue }
    } else if green < blue {
        green
    } else {
        blue
    };
    let lightness_sum = max + min;
    let distance_from_midpoint = if lightness_sum > 255 {
        lightness_sum - 255
    } else {
        255 - lightness_sum
    };
    let saturation = if saturation > 100 { 100 } else { saturation };
    let chroma = (255 - distance_from_midpoint) * saturation as u16 / 100;
    let hue_offset = hue % 120;
    let distance_from_sector_midpoint = if hue_offset > 60 {
        hue_offset - 60
    } else {
        60 - hue_offset
    };
    let secondary = chroma * (60 - distance_from_sector_midpoint) / 60;
    let (red_chroma, green_chroma, blue_chroma) = if hue < 60 {
        (chroma, secondary, 0)
    } else if hue < 120 {
        (secondary, chroma, 0)
    } else if hue < 180 {
        (0, chroma, secondary)
    } else if hue < 240 {
        (0, secondary, chroma)
    } else if hue < 300 {
        (secondary, 0, chroma)
    } else {
        (chroma, 0, secondary)
    };
    let doubled_offset = lightness_sum - chroma;
    (
        ((red_chroma * 2 + doubled_offset) / 2) as u8,
        ((green_chroma * 2 + doubled_offset) / 2) as u8,
        ((blue_chroma * 2 + doubled_offset) / 2) as u8,
    )
}

const fn brighten_channel(value: u8, amount: u16) -> u8 {
    let brightened = value as u16 + amount;
    if brightened > 255 {
        255
    } else {
        brightened as u8
    }
}

const fn palette(
    mode: ThemeMode,
    accent_color: AccentColor,
    background_hue: BackgroundHue,
    background_intensity: u8,
) -> Theme {
    let accent = accent_color.rgb();
    let is_dark = matches!(mode, ThemeMode::Dark | ThemeMode::Black);
    let (bg, panel, titlebar, hover, divider, text, muted, badge) = match mode {
        ThemeMode::Light => (
            (246, 247, 251),
            (255, 255, 255),
            (238, 239, 245),
            (234, 236, 243),
            (220, 222, 230),
            (32, 34, 43),
            (105, 110, 125),
            (231, 233, 240),
        ),
        ThemeMode::Dark => (
            (22, 22, 29),
            (30, 30, 40),
            (18, 18, 24),
            (40, 40, 54),
            (42, 42, 55),
            (235, 235, 245),
            (150, 150, 170),
            (52, 52, 70),
        ),
        ThemeMode::Black => (
            (0, 0, 0),
            (10, 10, 12),
            (0, 0, 0),
            (21, 21, 25),
            (32, 32, 36),
            (238, 238, 244),
            (145, 145, 158),
            (30, 30, 34),
        ),
    };
    let hue = background_hue.degrees();
    let saturation = if matches!(background_hue, BackgroundHue::Gray) {
        0
    } else {
        background_intensity
    };
    let bg = set_hue_and_saturation(bg, hue, saturation);
    let panel = set_hue_and_saturation(panel, hue, saturation);
    let titlebar = set_hue_and_saturation(titlebar, hue, saturation);
    let hover = set_hue_and_saturation(hover, hue, saturation);
    let divider = set_hue_and_saturation(divider, hue, saturation);
    let badge = set_hue_and_saturation(badge, hue, saturation);
    let selected = if matches!(mode, ThemeMode::Dark) && matches!(accent_color, AccentColor::Purple)
    {
        (47, 44, 82)
    } else {
        mix(bg, accent, if is_dark { 24 } else { 14 })
    };
    let accent_hover =
        if matches!(mode, ThemeMode::Dark) && matches!(accent_color, AccentColor::Purple) {
            (124, 96, 255)
        } else {
            let amount = if is_dark { 28 } else { 12 };
            (
                brighten_channel(accent.0, amount),
                brighten_channel(accent.1, amount),
                brighten_channel(accent.2, amount),
            )
        };
    let hover_strong = if is_dark {
        (50, 50, 68)
    } else {
        (224, 226, 234)
    };
    Theme {
        bg,
        panel,
        titlebar,
        selected,
        hover,
        hover_strong: set_hue_and_saturation(hover_strong, hue, saturation),
        close_hover: (150, 35, 35),
        text,
        muted,
        accent,
        accent_text: (255, 255, 255),
        accent_hover,
        divider,
        badge,
        danger: (220, 70, 90),
        danger_bg: match mode {
            ThemeMode::Light => (255, 235, 237),
            ThemeMode::Dark => (60, 20, 30),
            ThemeMode::Black => (42, 15, 20),
        },
        warning: (240, 140, 40),
        warning_bg: match mode {
            ThemeMode::Light => (255, 244, 224),
            ThemeMode::Dark => (50, 40, 28),
            ThemeMode::Black => (36, 29, 18),
        },
        success: (80, 200, 120),
    }
}

const INTENSITY_LEVELS: usize = 101;
const PALETTE_COUNT: usize = 192 * INTENSITY_LEVELS;

const fn palettes() -> [Theme; PALETTE_COUNT] {
    let mut result = [palette(
        ThemeMode::Dark,
        AccentColor::Purple,
        BackgroundHue::Purple,
        100,
    ); PALETTE_COUNT];
    let mut mode = 0;
    while mode < 3 {
        let mut accent = 0;
        while accent < 8 {
            let mut hue = 0;
            let theme_mode = match mode {
                0 => ThemeMode::Light,
                1 => ThemeMode::Dark,
                _ => ThemeMode::Black,
            };
            let accent_color = match accent {
                0 => AccentColor::Red,
                1 => AccentColor::Yellow,
                2 => AccentColor::Green,
                3 => AccentColor::Cyan,
                4 => AccentColor::Blue,
                5 => AccentColor::Purple,
                6 => AccentColor::Pink,
                _ => AccentColor::Gray,
            };
            while hue < 8 {
                let background_hue = match hue {
                    0 => BackgroundHue::Red,
                    1 => BackgroundHue::Yellow,
                    2 => BackgroundHue::Green,
                    3 => BackgroundHue::Cyan,
                    4 => BackgroundHue::Blue,
                    5 => BackgroundHue::Purple,
                    6 => BackgroundHue::Pink,
                    _ => BackgroundHue::Gray,
                };
                let mut intensity = 0;
                while intensity < INTENSITY_LEVELS {
                    let palette_index =
                        (mode * 64 + accent * 8 + hue) * INTENSITY_LEVELS + intensity;
                    result[palette_index] =
                        palette(theme_mode, accent_color, background_hue, intensity as u8);
                    intensity += 1;
                }
                hue += 1;
            }
            accent += 1;
        }
        mode += 1;
    }
    result
}

static PALETTES: [Theme; PALETTE_COUNT] = palettes();
static CURRENT_PALETTE: AtomicUsize = AtomicUsize::new(109 * INTENSITY_LEVELS + 15);

pub fn set_current(
    mode: ThemeMode,
    accent: AccentColor,
    background_hue: BackgroundHue,
    background_intensity: u8,
) {
    CURRENT_PALETTE.store(
        (mode.index() * 64 + accent.index() * 8 + background_hue.index()) * INTENSITY_LEVELS
            + usize::from(background_intensity.min(100)),
        Ordering::Relaxed,
    );
}

pub fn current_palette_index() -> usize {
    CURRENT_PALETTE.load(Ordering::Relaxed)
}

pub struct ThemeProxy;

impl Deref for ThemeProxy {
    type Target = Theme;

    fn deref(&self) -> &Self::Target {
        &PALETTES[current_palette_index()]
    }
}

pub static DARK: ThemeProxy = ThemeProxy;

#[cfg(test)]
mod tests {
    use super::{AccentColor, BackgroundHue, ThemeMode, palette};

    fn hsl_saturation(rgb: super::Rgb) -> f64 {
        let max = f64::from(rgb.0.max(rgb.1).max(rgb.2));
        let min = f64::from(rgb.0.min(rgb.1).min(rgb.2));
        let chroma = max - min;
        if chroma == 0.0 {
            0.0
        } else {
            chroma / (255.0 - (max + min - 255.0).abs())
        }
    }

    fn hsv_saturation(rgb: super::Rgb) -> f64 {
        let max = f64::from(rgb.0.max(rgb.1).max(rgb.2));
        let min = f64::from(rgb.0.min(rgb.1).min(rgb.2));
        (max - min) / max
    }

    fn hsl_lightness(rgb: super::Rgb) -> f64 {
        let max = f64::from(rgb.0.max(rgb.1).max(rgb.2));
        let min = f64::from(rgb.0.min(rgb.1).min(rgb.2));
        (max + min) / 510.0
    }

    #[test]
    fn zero_background_intensity_removes_hue_without_changing_accent() {
        let theme = palette(ThemeMode::Dark, AccentColor::Purple, BackgroundHue::Blue, 0);
        assert_eq!(theme.bg.0, theme.bg.1);
        assert_eq!(theme.bg.1, theme.bg.2);
        assert_eq!(theme.panel.0, theme.panel.1);
        assert_eq!(theme.panel.1, theme.panel.2);
        assert_eq!(theme.accent, (109, 74, 255));
    }

    #[test]
    fn background_hue_options_match_accent_options_and_order() {
        assert_eq!(
            AccentColor::ALL.map(|(_, title, _)| title),
            BackgroundHue::ALL.map(|(_, title, _)| title)
        );
    }

    #[test]
    fn palette_modes_and_accents_select_distinct_colors() {
        let light = palette(
            ThemeMode::Light,
            AccentColor::Red,
            BackgroundHue::Purple,
            71,
        );
        assert!(light.bg.2 > light.bg.0);
        assert_eq!(light.accent, AccentColor::Red.rgb());

        let black = palette(
            ThemeMode::Black,
            AccentColor::Cyan,
            BackgroundHue::Purple,
            71,
        );
        assert_eq!(black.bg, (0, 0, 0));
        assert_eq!(black.accent, AccentColor::Cyan.rgb());
    }

    #[test]
    fn accent_text_has_sufficient_contrast_for_every_accent_and_theme() {
        for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Black] {
            for (accent, _, _) in AccentColor::ALL {
                let theme = palette(mode, accent, BackgroundHue::Purple, 15);
                assert_eq!(theme.accent_text, (255, 255, 255));
                assert!(
                    contrast_ratio(theme.accent_text, theme.accent) >= 4.5,
                    "{mode:?} {accent:?} has insufficient accent text contrast"
                );
            }
        }
    }

    fn contrast_ratio(foreground: super::Rgb, background: super::Rgb) -> f64 {
        let luminance = |(red, green, blue): super::Rgb| {
            let linearize = |channel: u8| {
                let channel = f64::from(channel) / 255.0;
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linearize(red) + 0.7152 * linearize(green) + 0.0722 * linearize(blue)
        };
        let foreground_luminance = luminance(foreground);
        let background_luminance = luminance(background);
        (foreground_luminance.max(background_luminance) + 0.05)
            / (foreground_luminance.min(background_luminance) + 0.05)
    }

    #[test]
    fn background_hue_replaces_surface_hue_without_changing_lightness() {
        let red = palette(ThemeMode::Dark, AccentColor::Purple, BackgroundHue::Red, 71);
        let blue = palette(
            ThemeMode::Dark,
            AccentColor::Purple,
            BackgroundHue::Blue,
            71,
        );

        assert!(red.bg.0 > red.bg.1 && red.bg.0 > red.bg.2);
        assert!(blue.bg.2 > blue.bg.0 && blue.bg.2 > blue.bg.1);
        assert!((hsl_lightness(red.bg) - hsl_lightness(blue.bg)).abs() < 0.01);
        assert_eq!(blue.accent, red.accent);
    }

    #[test]
    fn background_intensity_controls_hue_saturation() {
        let no_saturation = palette(
            ThemeMode::Light,
            AccentColor::Purple,
            BackgroundHue::Blue,
            0,
        );
        let half = palette(
            ThemeMode::Light,
            AccentColor::Purple,
            BackgroundHue::Blue,
            50,
        );
        let full = palette(
            ThemeMode::Light,
            AccentColor::Purple,
            BackgroundHue::Blue,
            100,
        );

        assert_eq!(no_saturation.bg.0, no_saturation.bg.1);
        assert_eq!(no_saturation.bg.1, no_saturation.bg.2);
        assert!((hsl_saturation(half.bg) - 0.5).abs() < 0.1);
        assert!((hsl_saturation(full.bg) - 1.0).abs() < 0.1);
        assert!((hsl_lightness(half.bg) - hsl_lightness(full.bg)).abs() < 0.01);
        assert_eq!(half.accent, full.accent);
    }

    #[test]
    fn gray_background_hue_stays_neutral_at_any_intensity() {
        let low = palette(
            ThemeMode::Dark,
            AccentColor::Purple,
            BackgroundHue::Gray,
            15,
        );
        let high = palette(
            ThemeMode::Dark,
            AccentColor::Purple,
            BackgroundHue::Gray,
            100,
        );

        assert_eq!(low.bg.0, low.bg.1);
        assert_eq!(low.bg.1, low.bg.2);
        assert_eq!(low.bg, high.bg);
        assert_eq!(low.panel, high.panel);
    }

    #[test]
    fn background_hue_swatches_match_purple_saturation_and_lightness() {
        let purple = BackgroundHue::Purple.rgb();
        let purple_saturation = hsl_saturation(purple);
        let purple_lightness = hsl_lightness(purple);

        for (hue, _, rgb) in BackgroundHue::ALL {
            if matches!(hue, BackgroundHue::Gray) {
                assert_eq!(hsl_saturation(rgb), 0.0);
                assert!(
                    (hsl_lightness(rgb) - purple_lightness).abs() < 0.005,
                    "{hue:?} lightness differs from purple"
                );
                continue;
            }
            assert!(
                (hsv_saturation(rgb) - 0.71).abs() < 0.005,
                "{hue:?} HSV saturation differs from purple"
            );
            assert!(
                (hsl_lightness(rgb) - purple_lightness).abs() < 0.005,
                "{hue:?} lightness differs from purple"
            );
            assert!(
                (hsl_saturation(rgb) - purple_saturation).abs() < 0.005,
                "{hue:?} saturation differs from purple"
            );
        }
    }
}
