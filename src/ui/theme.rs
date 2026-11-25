use ratatui::style::Color;

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub panel: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub accent2: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
}

pub const PRESETS: [&str; 5] = ["erdtree", "blood", "moon", "ash", "mono"];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

pub fn next_theme(current: &str) -> &'static str {
    let idx = PRESETS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    PRESETS[(idx + 1) % PRESETS.len()]
}

impl Theme {
    pub fn by_name(name: &str) -> Theme {
        let theme = match name.trim().to_ascii_lowercase().as_str() {
            "blood" => Theme {
                name: "blood",
                bg: Color::Reset,
                panel: Color::Reset,
                text: rgb(255, 232, 232),
                muted: rgb(150, 96, 96),
                border: rgb(84, 30, 30),
                accent: rgb(232, 44, 62),
                accent2: rgb(255, 176, 120),
                good: rgb(126, 231, 135),
                warn: rgb(255, 196, 0),
                bad: rgb(255, 90, 95),
            },
            "moon" => Theme {
                name: "moon",
                bg: Color::Reset,
                panel: Color::Reset,
                text: rgb(224, 240, 255),
                muted: rgb(104, 128, 158),
                border: rgb(36, 52, 84),
                accent: rgb(126, 182, 255),
                accent2: rgb(196, 168, 255),
                good: rgb(110, 231, 183),
                warn: rgb(250, 204, 21),
                bad: rgb(248, 113, 113),
            },
            "ash" => Theme {
                name: "ash",
                bg: Color::Reset,
                panel: Color::Reset,
                text: rgb(230, 228, 224),
                muted: rgb(132, 128, 122),
                border: rgb(64, 62, 58),
                accent: rgb(214, 210, 200),
                accent2: rgb(255, 138, 92),
                good: rgb(160, 220, 160),
                warn: rgb(240, 200, 120),
                bad: rgb(230, 110, 110),
            },
            "mono" => Theme {
                name: "mono",
                bg: Color::Reset,
                panel: Color::Reset,
                text: rgb(235, 235, 235),
                muted: rgb(128, 128, 128),
                border: rgb(64, 64, 64),
                accent: rgb(255, 255, 255),
                accent2: rgb(184, 184, 184),
                good: rgb(232, 232, 232),
                warn: rgb(200, 200, 200),
                bad: rgb(255, 255, 255),
            },
            _ => Theme {
                name: "erdtree",
                bg: Color::Reset,
                panel: Color::Reset,
                text: rgb(240, 232, 212),
                muted: rgb(148, 130, 96),
                border: rgb(86, 70, 40),
                accent: rgb(232, 195, 107),
                accent2: rgb(167, 214, 190),
                good: rgb(150, 224, 130),
                warn: rgb(255, 200, 92),
                bad: rgb(255, 104, 96),
            },
        };
        theme.adapted()
    }

    fn adapted(mut self) -> Theme {
        self.text = Color::Reset;
        if light_background() {
            self.accent = shade(self.accent, 0.55);
            self.accent2 = shade(self.accent2, 0.55);
            self.muted = shade(self.muted, 0.65);
            self.good = shade(self.good, 0.60);
            self.warn = shade(self.warn, 0.70);
            self.bad = shade(self.bad, 0.85);
        }
        self
    }

    pub fn grad(&self, t: f32) -> Color {
        lerp(self.accent, self.accent2, t.clamp(0.0, 1.0))
    }
}

pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => Color::Rgb(
            (ar as f32 + (br as f32 - ar as f32) * t) as u8,
            (ag as f32 + (bg as f32 - ag as f32) * t) as u8,
            (ab as f32 + (bb as f32 - ab as f32) * t) as u8,
        ),
        _ => a,
    }
}

fn light_background() -> bool {
    std::env::var("COLORFGBG")
        .ok()
        .and_then(|value| {
            value
                .rsplit(';')
                .next()
                .and_then(|n| n.trim().parse::<u8>().ok())
        })
        .is_some_and(|bg| bg >= 8)
}

pub fn shade(color: Color, factor: f32) -> Color {
    let f = factor.clamp(0.0, 1.0);
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as f32 * f) as u8,
            (g as f32 * f) as u8,
            (b as f32 * f) as u8,
        ),
        other => other,
    }
}
