use std::io::IsTerminal;

// Aurora design-system CLI tokens (dark-first, operator-grade palette).
const PRIMARY: (u8, u8, u8) = (0xe6, 0xf4, 0xfb);
const MUTED: (u8, u8, u8) = (0xa7, 0xbc, 0xc9);
const CYAN: (u8, u8, u8) = (0x29, 0xb6, 0xf6);
const SUCCESS: (u8, u8, u8) = (0x7d, 0xd3, 0xc7);
const WARN: (u8, u8, u8) = (0xc6, 0xa3, 0x6b);
const ERROR: (u8, u8, u8) = (0xc7, 0x84, 0x90);
const VIOLET: (u8, u8, u8) = (0xa7, 0x8b, 0xfa);

fn paint(rgb: (u8, u8, u8), text: &str) -> String {
    format!("\x1b[38;2;{};{};{}m{}\x1b[0m", rgb.0, rgb.1, rgb.2, text)
}

fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

pub struct Palette {
    enabled: bool,
}

#[allow(dead_code)]
impl Palette {
    pub fn new() -> Self {
        Self {
            enabled: color_enabled(),
        }
    }

    pub fn plain() -> Self {
        Self { enabled: false }
    }

    pub fn primary<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.enabled {
            paint(PRIMARY, text).into()
        } else {
            text.into()
        }
    }

    pub fn muted<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.enabled {
            paint(MUTED, text).into()
        } else {
            text.into()
        }
    }

    pub fn cyan<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.enabled {
            paint(CYAN, text).into()
        } else {
            text.into()
        }
    }

    pub fn success<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.enabled {
            paint(SUCCESS, text).into()
        } else {
            text.into()
        }
    }

    pub fn warn<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.enabled {
            paint(WARN, text).into()
        } else {
            text.into()
        }
    }

    pub fn error<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.enabled {
            paint(ERROR, text).into()
        } else {
            text.into()
        }
    }

    pub fn violet<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.enabled {
            paint(VIOLET, text).into()
        } else {
            text.into()
        }
    }

    pub fn severity<'a>(&self, sev: &'a str) -> std::borrow::Cow<'a, str> {
        if !self.enabled {
            return sev.into();
        }
        let lower = sev.to_ascii_lowercase();
        if lower.starts_with("err") || lower == "crit" || lower == "alert" || lower == "emerg" {
            paint(ERROR, sev).into()
        } else if lower.starts_with("warn") {
            paint(WARN, sev).into()
        } else if lower == "info" || lower == "notice" {
            paint(SUCCESS, sev).into()
        } else {
            paint(MUTED, sev).into()
        }
    }
}
