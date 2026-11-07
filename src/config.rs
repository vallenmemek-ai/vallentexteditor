//! Pengaturan sederhana disimpan di ~/.config/vallentext/config.ini (format key=value).
//! Tidak pakai crate tambahan supaya binary tetap kecil.

use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub dark: bool,
    pub wrap: bool,
    pub line_numbers: bool,
    pub statusbar: bool,
    pub highlight_line: bool,
    pub use_spaces: bool,
    pub tab_width: u32,
    pub font_size: u32, // dalam pt
    pub win_w: i32,
    pub win_h: i32,
    pub maximized: bool,
    pub trim_trailing: bool,
    pub final_newline: bool,
    /// ID skema warna GtkSourceView, atau "auto" (ikut tema gelap/terang).
    pub scheme: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            dark: true,
            wrap: false,
            line_numbers: true,
            statusbar: true,
            highlight_line: true,
            use_spaces: true,
            tab_width: 4,
            font_size: 11,
            win_w: 900,
            win_h: 600,
            maximized: false,
            trim_trailing: false,
            final_newline: false,
            scheme: "auto".to_string(),
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("vallentext").join("config.ini"))
}

impl Config {
    pub fn load() -> Config {
        let mut c = Config::default();
        let Some(p) = path() else { return c };
        let Ok(text) = fs::read_to_string(p) else { return c };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let b = v == "true";
            match k {
                "dark" => c.dark = b,
                "wrap" => c.wrap = b,
                "line_numbers" => c.line_numbers = b,
                "statusbar" => c.statusbar = b,
                "highlight_line" => c.highlight_line = b,
                "use_spaces" => c.use_spaces = b,
                "maximized" => c.maximized = b,
                "trim_trailing" => c.trim_trailing = b,
                "final_newline" => c.final_newline = b,
                "scheme" => {
                    if !v.is_empty() {
                        c.scheme = v.to_string();
                    }
                }
                "tab_width" => c.tab_width = v.parse().unwrap_or(4).clamp(1, 16),
                "font_size" => c.font_size = v.parse().unwrap_or(11).clamp(6, 48),
                "win_w" => c.win_w = v.parse().unwrap_or(900).max(300),
                "win_h" => c.win_h = v.parse().unwrap_or(600).max(200),
                _ => {}
            }
        }
        c
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let s = format!(
            "dark={}\nwrap={}\nline_numbers={}\nstatusbar={}\nhighlight_line={}\nuse_spaces={}\ntab_width={}\nfont_size={}\nwin_w={}\nwin_h={}\nmaximized={}\ntrim_trailing={}\nfinal_newline={}\nscheme={}\n",
            self.dark, self.wrap, self.line_numbers, self.statusbar, self.highlight_line,
            self.use_spaces, self.tab_width, self.font_size, self.win_w, self.win_h, self.maximized,
            self.trim_trailing, self.final_newline, self.scheme
        );
        let _ = fs::write(p, s);
    }
}
