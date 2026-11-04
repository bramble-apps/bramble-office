//! The looks: iOS 26 in Dieter Rams' Braun ET66 colours by default, today's iOS (26, Liquid Glass), and the calculators of earlier years
//! (iOS 18 / macOS 2024, macOS 2019, iPhone 2007, Mac OS X 2001, Macintosh 1984).
//! Colours are 0xRRGGBBAA.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Family {
    /// iOS 26: ⌫ C % ÷ … +/- 0 . =, history and mode buttons in the top bar
    Modern26,
    /// iOS 18: AC/⌫ +/- % ÷ … [calc] 0 . =, history list icon at the top left
    Modern18,
    /// macOS Mojave: one flat panel, keys separated by hairlines
    Mac2019,
    /// iPhone OS 1: memory row, glossy keys, tall =
    Iphone2007,
    /// Mac OS X 10.0: Aqua gel keys, LCD
    Aqua2001,
    /// Macintosh desk accessory: black on white
    Mac1984,
}

impl Family {
    pub fn classic(self) -> bool {
        !matches!(self, Family::Modern26 | Family::Modern18)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Press {
    /// Liquid Glass: the key brightens and swells, then springs back
    Glass,
    /// lights up at once, fades back out on release
    Fade,
    /// colours swap while held
    Invert,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Shape {
    /// circle when square, capsule when wider
    Round,
    /// rounded rectangle; radius as a fraction of the shorter side
    RoundRect(f32),
    Rect,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct KeyStyle {
    pub fill: u32,
    pub fill2: u32,
    pub pressed: u32,
    pub text: u32,
    pub border: u32,
    /// the shiny top half of glossy keys
    pub gloss: bool,
    /// a hard offset shadow (1984) or soft drop shadow
    pub shadow: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct DisplayStyle {
    pub bg: u32,
    pub bg2: u32,
    pub text: u32,
    pub expr: u32,
    pub border: u32,
    pub radius: f32,
    /// the number sits in a framed box (LCD) instead of on the background
    pub boxed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub year: &'static str,
    pub family: Family,
    pub bg: u32,
    pub bg2: u32,
    /// panel behind the keys on device-like themes (0 = none)
    pub body: u32,
    pub body2: u32,
    pub body_border: u32,
    pub display: DisplayStyle,
    pub digit: KeyStyle,
    pub func: KeyStyle,
    pub op: KeyStyle,
    pub sci: KeyStyle,
    pub eq: KeyStyle,
    pub mem: KeyStyle,
    /// 2nd / Rad switched on, or the waiting operator on classic themes
    pub active: KeyStyle,
    pub shape: Shape,
    /// gap between keys as a fraction of a key's width
    pub gap: f32,
    pub press: Press,
    /// toolbar icons and accents
    pub accent: u32,
    /// icons sit in dark glass circles (iOS 26)
    pub glass_toolbar: bool,
    /// sheets, menus, history panel
    pub panel: u32,
    pub panel_text: u32,
    pub panel_dim: u32,
    pub separator: u32,
    /// font: "system" (thin digits), "rounded", "mono", "chicago"
    pub font: &'static str,
    /// weight of the big number (100..900)
    pub result_weight: u16,
    pub label_weight: u16,
    /// key labels use * / - for operators (1984)
    pub ascii_ops: bool,
    /// the decimal point takes the operator keys' colour (Braun)
    pub dot_op: bool,
    pub light: bool,
}

const fn ks(fill: u32, pressed: u32, text: u32) -> KeyStyle {
    KeyStyle { fill, fill2: fill, pressed, text, border: 0, gloss: false, shadow: 0 }
}

const fn grad(fill: u32, fill2: u32, pressed: u32, text: u32, border: u32, gloss: bool) -> KeyStyle {
    KeyStyle { fill, fill2, pressed, text, border, gloss, shadow: 0 }
}

pub fn themes() -> Vec<Theme> {
    let orange = 0xFF9F0AFF;
    let orange_p = 0xFCC78DFF;
    vec![
        Theme {
            id: "braun",
            name: "Braun",
            year: "1987",
            family: Family::Modern26,
            bg: 0x000000FF,
            bg2: 0x000000FF,
            body: 0,
            body2: 0,
            body_border: 0,
            display: DisplayStyle { bg: 0, bg2: 0, text: 0xFFFFFFFF, expr: 0x8E8E93FF, border: 0, radius: 0.0, boxed: false },
            digit: grad(0x3A3A3CFF, 0x2C2C2EFF, 0x6A6A6EFF, 0xFFFFFFFF, 0xFFFFFF1A, false),
            func: grad(0x17301FFF, 0x0E2114FF, 0x2F5439FF, 0xFFFFFFFF, 0xFFFFFF22, false),
            op: grad(0x2E1D13FF, 0x1F120BFF, 0x55392AFF, 0xFFFFFFFF, 0xFFFFFF26, false),
            sci: grad(0x2A2A2CFF, 0x1E1E20FF, 0x56565AFF, 0xFFFFFFFF, 0xFFFFFF14, false),
            eq: grad(0xF7B044FF, 0xEE9426FF, 0xFBD08DFF, 0x000000FF, 0xFFFFFF33, false),
            mem: grad(0x2A2A2CFF, 0x1E1E20FF, 0x56565AFF, 0xFFFFFFFF, 0xFFFFFF14, false),
            active: grad(0x8E8E93FF, 0x7C7C82FF, 0xB0B0B6FF, 0xFFFFFFFF, 0xFFFFFF22, false),
            shape: Shape::Round,
            gap: 0.2,
            press: Press::Glass,
            accent: 0xEE9426FF,
            glass_toolbar: true,
            panel: 0x1C1C1EFF,
            panel_text: 0xFFFFFFFF,
            panel_dim: 0x8E8E93FF,
            separator: 0x38383AFF,
            font: "system",
            result_weight: 300,
            label_weight: 400,
            ascii_ops: false,
            dot_op: true,
            light: false,
        },
        Theme {
            id: "ios26",
            name: "iOS 26",
            year: "2025",
            family: Family::Modern26,
            bg: 0x000000FF,
            bg2: 0x000000FF,
            body: 0,
            body2: 0,
            body_border: 0,
            display: DisplayStyle { bg: 0, bg2: 0, text: 0xFFFFFFFF, expr: 0x8E8E93FF, border: 0, radius: 0.0, boxed: false },
            digit: grad(0x3A3A3CFF, 0x2C2C2EFF, 0x6A6A6EFF, 0xFFFFFFFF, 0xFFFFFF1A, false),
            func: grad(0x6E6E73FF, 0x5C5C61FF, 0x9A9AA0FF, 0xFFFFFFFF, 0xFFFFFF22, false),
            op: grad(0xFFA826FF, 0xFF9500FF, orange_p, 0xFFFFFFFF, 0xFFFFFF33, false),
            sci: grad(0x2A2A2CFF, 0x1E1E20FF, 0x56565AFF, 0xFFFFFFFF, 0xFFFFFF14, false),
            eq: grad(0xFFA826FF, 0xFF9500FF, orange_p, 0xFFFFFFFF, 0xFFFFFF33, false),
            mem: grad(0x2A2A2CFF, 0x1E1E20FF, 0x56565AFF, 0xFFFFFFFF, 0xFFFFFF14, false),
            active: grad(0x8E8E93FF, 0x7C7C82FF, 0xB0B0B6FF, 0xFFFFFFFF, 0xFFFFFF22, false),
            shape: Shape::Round,
            gap: 0.2,
            press: Press::Glass,
            accent: orange,
            glass_toolbar: true,
            panel: 0x1C1C1EFF,
            panel_text: 0xFFFFFFFF,
            panel_dim: 0x8E8E93FF,
            separator: 0x38383AFF,
            font: "system",
            result_weight: 300,
            label_weight: 400,
            ascii_ops: false,
            dot_op: false,
            light: false,
        },
        Theme {
            id: "ios18",
            name: "iOS 18",
            year: "2024",
            family: Family::Modern18,
            bg: 0x000000FF,
            bg2: 0x000000FF,
            body: 0,
            body2: 0,
            body_border: 0,
            display: DisplayStyle { bg: 0, bg2: 0, text: 0xFFFFFFFF, expr: 0x8E8E93FF, border: 0, radius: 0.0, boxed: false },
            digit: ks(0x333333FF, 0x737373FF, 0xFFFFFFFF),
            func: ks(0x5C5C5FFF, 0x8C8C8FFF, 0xFFFFFFFF),
            op: ks(orange, orange_p, 0xFFFFFFFF),
            sci: ks(0x1C1C1CFF, 0x4D4D4DFF, 0xFFFFFFFF),
            eq: ks(orange, orange_p, 0xFFFFFFFF),
            mem: ks(0x1C1C1CFF, 0x4D4D4DFF, 0xFFFFFFFF),
            active: ks(0x7C7C7FFF, 0xA5A5A5FF, 0xFFFFFFFF),
            shape: Shape::Round,
            gap: 0.2,
            press: Press::Fade,
            accent: orange,
            glass_toolbar: false,
            panel: 0x1C1C1EFF,
            panel_text: 0xFFFFFFFF,
            panel_dim: 0x8E8E93FF,
            separator: 0x38383AFF,
            font: "system",
            result_weight: 300,
            label_weight: 400,
            ascii_ops: false,
            dot_op: false,
            light: false,
        },
        Theme {
            id: "mac2019",
            name: "macOS Mojave",
            year: "2019",
            family: Family::Mac2019,
            bg: 0x1E1E1EFF,
            bg2: 0x1E1E1EFF,
            body: 0x2B2B2BFF,
            body2: 0x2B2B2BFF,
            body_border: 0x111111FF,
            display: DisplayStyle { bg: 0x3A3A3CFF, bg2: 0x333335FF, text: 0xFFFFFFFF, expr: 0xA0A0A0FF, border: 0, radius: 0.0, boxed: false },
            digit: ks(0x6E6E70FF, 0x9E9EA0FF, 0xFFFFFFFF),
            func: ks(0x58585AFF, 0x88888AFF, 0xFFFFFFFF),
            op: ks(0xFF9F0AFF, 0xC77700FF, 0xFFFFFFFF),
            sci: ks(0x58585AFF, 0x88888AFF, 0xFFFFFFFF),
            eq: ks(0xFF9F0AFF, 0xC77700FF, 0xFFFFFFFF),
            mem: ks(0x58585AFF, 0x88888AFF, 0xFFFFFFFF),
            active: ks(0xC77700FF, 0xC77700FF, 0xFFFFFFFF),
            shape: Shape::Rect,
            gap: 0.012,
            press: Press::Fade,
            accent: 0xFF9F0AFF,
            glass_toolbar: false,
            panel: 0x2B2B2BFF,
            panel_text: 0xFFFFFFFF,
            panel_dim: 0x9A9A9AFF,
            separator: 0x444444FF,
            font: "system",
            result_weight: 200,
            label_weight: 300,
            ascii_ops: false,
            dot_op: false,
            light: false,
        },
        Theme {
            id: "iphone2007",
            name: "iPhone",
            year: "2007",
            family: Family::Iphone2007,
            bg: 0x1A1A1AFF,
            bg2: 0x050505FF,
            body: 0x2A2B2EFF,
            body2: 0x0E0F10FF,
            body_border: 0x000000FF,
            display: DisplayStyle { bg: 0xD5DCCBFF, bg2: 0xA7AF9CFF, text: 0x101010FF, expr: 0x404040FF, border: 0x2B2B2BFF, radius: 0.12, boxed: true },
            digit: grad(0x4B4E54FF, 0x1C1E21FF, 0x7A7F88FF, 0xFFFFFFFF, 0x000000FF, true),
            func: grad(0x6F7685FF, 0x3B414DFF, 0x9AA2B4FF, 0xFFFFFFFF, 0x000000FF, true),
            op: grad(0x6F7685FF, 0x3B414DFF, 0x9AA2B4FF, 0xFFFFFFFF, 0x000000FF, true),
            sci: grad(0x6F7685FF, 0x3B414DFF, 0x9AA2B4FF, 0xFFFFFFFF, 0x000000FF, true),
            eq: grad(0xFFA43AFF, 0xE06A00FF, 0xFFC27AFF, 0xFFFFFFFF, 0x000000FF, true),
            mem: grad(0x6F7685FF, 0x3B414DFF, 0x9AA2B4FF, 0xFFFFFFFF, 0x000000FF, true),
            active: grad(0xA9B2C6FF, 0x6D7690FF, 0xC2C9D8FF, 0xFFFFFFFF, 0x000000FF, true),
            shape: Shape::RoundRect(0.14),
            gap: 0.09,
            press: Press::Fade,
            accent: 0xF89A2CFF,
            glass_toolbar: false,
            panel: 0x2A2B2EFF,
            panel_text: 0xFFFFFFFF,
            panel_dim: 0x9A9EA6FF,
            separator: 0x45484EFF,
            font: "system",
            result_weight: 700,
            label_weight: 700,
            ascii_ops: false,
            dot_op: false,
            light: false,
        },
        Theme {
            id: "aqua2001",
            name: "Mac OS X",
            year: "2001",
            family: Family::Aqua2001,
            bg: 0xE8E8E8FF,
            bg2: 0xD0D0D0FF,
            body: 0xEDEDEDFF,
            body2: 0xD6D6D6FF,
            body_border: 0x8A8A8AFF,
            display: DisplayStyle { bg: 0xD9E6C2FF, bg2: 0xB9C9A0FF, text: 0x1E2A12FF, expr: 0x50603AFF, border: 0x6F7F5AFF, radius: 0.08, boxed: true },
            digit: grad(0xFFFFFFFF, 0xD3D3D3FF, 0x9FC6F2FF, 0x111111FF, 0x8C8C8CFF, true),
            func: grad(0xF4F4F4FF, 0xC4C4C4FF, 0x9FC6F2FF, 0x111111FF, 0x8C8C8CFF, true),
            op: grad(0xF4F4F4FF, 0xC4C4C4FF, 0x9FC6F2FF, 0x111111FF, 0x8C8C8CFF, true),
            sci: grad(0xF4F4F4FF, 0xC4C4C4FF, 0x9FC6F2FF, 0x111111FF, 0x8C8C8CFF, true),
            eq: grad(0xF4F4F4FF, 0xC4C4C4FF, 0x9FC6F2FF, 0x111111FF, 0x8C8C8CFF, true),
            mem: grad(0xF4F4F4FF, 0xC4C4C4FF, 0x9FC6F2FF, 0x111111FF, 0x8C8C8CFF, true),
            active: grad(0xB9D9FBFF, 0x5E9FE6FF, 0x9FC6F2FF, 0x111111FF, 0x3B6EA8FF, true),
            shape: Shape::RoundRect(0.3),
            gap: 0.12,
            press: Press::Fade,
            accent: 0x3B7DD8FF,
            glass_toolbar: false,
            panel: 0xF2F2F2FF,
            panel_text: 0x111111FF,
            panel_dim: 0x6E6E6EFF,
            separator: 0xC8C8C8FF,
            font: "system",
            result_weight: 500,
            label_weight: 500,
            ascii_ops: false,
            dot_op: false,
            light: true,
        },
        Theme {
            id: "mac1984",
            name: "Macintosh",
            year: "1984",
            family: Family::Mac1984,
            bg: 0xFFFFFFFF,
            bg2: 0xFFFFFFFF,
            body: 0xFFFFFFFF,
            body2: 0xFFFFFFFF,
            body_border: 0x000000FF,
            display: DisplayStyle { bg: 0xFFFFFFFF, bg2: 0xFFFFFFFF, text: 0x000000FF, expr: 0x000000FF, border: 0x000000FF, radius: 0.0, boxed: true },
            digit: KeyStyle { fill: 0xFFFFFFFF, fill2: 0xFFFFFFFF, pressed: 0x000000FF, text: 0x000000FF, border: 0x000000FF, gloss: false, shadow: 0x000000FF },
            func: KeyStyle { fill: 0xFFFFFFFF, fill2: 0xFFFFFFFF, pressed: 0x000000FF, text: 0x000000FF, border: 0x000000FF, gloss: false, shadow: 0x000000FF },
            op: KeyStyle { fill: 0xFFFFFFFF, fill2: 0xFFFFFFFF, pressed: 0x000000FF, text: 0x000000FF, border: 0x000000FF, gloss: false, shadow: 0x000000FF },
            sci: KeyStyle { fill: 0xFFFFFFFF, fill2: 0xFFFFFFFF, pressed: 0x000000FF, text: 0x000000FF, border: 0x000000FF, gloss: false, shadow: 0x000000FF },
            eq: KeyStyle { fill: 0xFFFFFFFF, fill2: 0xFFFFFFFF, pressed: 0x000000FF, text: 0x000000FF, border: 0x000000FF, gloss: false, shadow: 0x000000FF },
            mem: KeyStyle { fill: 0xFFFFFFFF, fill2: 0xFFFFFFFF, pressed: 0x000000FF, text: 0x000000FF, border: 0x000000FF, gloss: false, shadow: 0x000000FF },
            active: KeyStyle { fill: 0x000000FF, fill2: 0x000000FF, pressed: 0x000000FF, text: 0xFFFFFFFF, border: 0x000000FF, gloss: false, shadow: 0x000000FF },
            shape: Shape::RoundRect(0.18),
            gap: 0.16,
            press: Press::Invert,
            accent: 0x000000FF,
            glass_toolbar: false,
            panel: 0xFFFFFFFF,
            panel_text: 0x000000FF,
            panel_dim: 0x555555FF,
            separator: 0x000000FF,
            font: "chicago",
            result_weight: 700,
            label_weight: 700,
            ascii_ops: true,
            dot_op: false,
            light: true,
        },
    ]
}

pub fn theme(id: &str) -> Theme {
    let all = themes();
    all.iter().find(|t| t.id == id).cloned().unwrap_or_else(|| all[0].clone())
}
