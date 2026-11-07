//! Satu dokumen = satu tab: buffer + view + info file.

use crate::config::Config;
use crate::file_io::{self, Encoding};
use gtk::prelude::*;
use sourceview4::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

pub struct Document {
    pub scroller: gtk::ScrolledWindow,
    pub view: sourceview4::View,
    pub buffer: sourceview4::Buffer,
    pub tab_label: gtk::Label,
    pub path: RefCell<Option<PathBuf>>,
    pub encoding: Cell<Encoding>,
    pub crlf: Cell<bool>,
    /// File aslinya campuran CRLF/LF (diseragamkan saat disimpan).
    pub mixed_eol: Cell<bool>,
    /// Waktu modifikasi file saat terakhir dibaca/disimpan (untuk deteksi perubahan di disk).
    pub mtime: Cell<Option<SystemTime>>,
    /// ID untuk salinan pemulihan (0 = belum diberi).
    pub rid: Cell<u32>,
    /// Isi berubah sejak autosave terakhir.
    pub dirty: Cell<bool>,
    pub untitled_name: String,
}

impl Document {
    pub fn new(untitled_name: String, cfg: &Config) -> Rc<Document> {
        let buffer = sourceview4::Buffer::new(None::<&gtk::TextTagTable>);
        buffer.set_highlight_matching_brackets(true);
        buffer.set_max_undo_levels(500);

        let view = sourceview4::View::with_buffer(&buffer);
        view.set_monospace(true);
        view.set_auto_indent(true);
        view.set_indent_on_tab(true);
        view.set_smart_home_end(sourceview4::SmartHomeEndType::Before);
        view.set_show_right_margin(false);
        view.set_left_margin(4);
        view.set_top_margin(2);
        view.set_bottom_margin(2);

        let scroller = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scroller.add(&view);

        let tab_label = gtk::Label::new(Some(&untitled_name));
        tab_label.set_ellipsize(pango_ellipsize_end());
        tab_label.set_max_width_chars(24);

        let doc = Rc::new(Document {
            scroller,
            view,
            buffer,
            tab_label,
            path: RefCell::new(None),
            encoding: Cell::new(Encoding::Utf8),
            crlf: Cell::new(false),
            mixed_eol: Cell::new(false),
            mtime: Cell::new(None),
            rid: Cell::new(0),
            dirty: Cell::new(false),
            untitled_name,
        });
        doc.apply_config(cfg);
        doc
    }

    pub fn apply_config(&self, cfg: &Config) {
        self.view.set_show_line_numbers(cfg.line_numbers);
        self.view.set_highlight_current_line(cfg.highlight_line);
        self.view.set_tab_width(cfg.tab_width);
        self.view.set_insert_spaces_instead_of_tabs(cfg.use_spaces);
        self.view.set_wrap_mode(if cfg.wrap { gtk::WrapMode::WordChar } else { gtk::WrapMode::None });
        let scheme_id = if cfg.scheme != "auto" && !cfg.scheme.is_empty() {
            cfg.scheme.as_str()
        } else if cfg.dark {
            "oblivion"
        } else {
            "classic"
        };
        let scheme = sourceview4::StyleSchemeManager::default().and_then(|m| m.scheme(scheme_id));
        self.buffer.set_style_scheme(scheme.as_ref());
    }

    pub fn display_name(&self) -> String {
        match &*self.path.borrow() {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
            None => self.untitled_name.clone(),
        }
    }

    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }

    pub fn is_pristine_untitled(&self) -> bool {
        self.path.borrow().is_none() && !self.is_modified() && self.buffer.char_count() == 0
    }

    pub fn set_language_by_path(&self, path: &Path) {
        let lang = sourceview4::LanguageManager::default().and_then(|lm| lm.guess_language(path.to_str(), None));
        self.buffer.set_language(lang.as_ref());
    }

    pub fn load(&self, path: &Path) -> Result<(), String> {
        self.load_as(path, None)
    }

    /// `forced`: paksa encoding tertentu (Buka Ulang dengan Encoding).
    pub fn load_as(&self, path: &Path, forced: Option<Encoding>) -> Result<(), String> {
        let loaded = file_io::read_text_as(path, forced)?;
        // File besar (>3 MB): matikan syntax highlighting supaya tetap ngebut.
        let big = loaded.text.len() > 3_000_000;
        self.buffer.set_highlight_syntax(!big);
        self.buffer.set_highlight_matching_brackets(!big);
        self.buffer.begin_not_undoable_action();
        self.buffer.set_text(&loaded.text);
        self.buffer.end_not_undoable_action();
        self.buffer.set_modified(false);
        self.encoding.set(loaded.encoding);
        self.crlf.set(loaded.crlf);
        self.mixed_eol.set(loaded.mixed_eol);
        self.mtime.set(file_mtime(path));
        *self.path.borrow_mut() = Some(path.to_path_buf());
        // Bahasa sintaks sengaja TIDAK dipasang di sini: pertama kali, GtkSourceView memindai
        // ratusan file .lang (lambat di CPU tua). Pemanggil memasangnya setelah teks tampil.
        let start = self.buffer.start_iter();
        self.buffer.place_cursor(&start);
        Ok(())
    }

    pub fn save_to(&self, path: &Path, cfg: &Config) -> Result<(), String> {
        self.apply_save_filters(cfg);
        let (s, e) = self.buffer.bounds();
        let text = self.buffer.text(&s, &e, false).ok_or("Gagal membaca isi buffer")?;
        file_io::write_text(path, text.as_str(), self.encoding.get(), self.crlf.get())?;
        let changed = self.path.borrow().as_deref() != Some(path);
        *self.path.borrow_mut() = Some(path.to_path_buf());
        self.buffer.set_modified(false);
        self.mixed_eol.set(false); // sudah diseragamkan oleh write_text
        self.mtime.set(file_mtime(path));
        if changed {
            self.set_language_by_path(path);
        }
        Ok(())
    }

    /// Isi dokumen dari salinan pemulihan: tampil sebagai "belum disimpan".
    pub fn restore(&self, path: Option<PathBuf>, text: &str) {
        self.buffer.begin_not_undoable_action();
        self.buffer.set_text(text);
        self.buffer.end_not_undoable_action();
        if let Some(p) = &path {
            self.set_language_by_path(p);
        }
        *self.path.borrow_mut() = path;
        let start = self.buffer.start_iter();
        self.buffer.place_cursor(&start);
        self.buffer.set_modified(true);
    }

    /// Opsi simpan: hapus spasi di ujung baris, akhiri file dengan baris baru.
    /// Dikerjakan di buffer (bisa di-undo) supaya yang tampil sama dengan yang tersimpan.
    fn apply_save_filters(&self, cfg: &Config) {
        if !cfg.trim_trailing && !cfg.final_newline {
            return;
        }
        let buf = &self.buffer;
        buf.begin_user_action();
        if cfg.trim_trailing {
            for line in (0..buf.line_count()).rev() {
                let mut end = buf.iter_at_line(line);
                if !end.ends_line() {
                    end.forward_to_line_end();
                }
                let mut start = end;
                while start.backward_char() {
                    match start.char() {
                        Some(' ') | Some('\t') => {}
                        _ => {
                            start.forward_char();
                            break;
                        }
                    }
                }
                if start.offset() < end.offset() {
                    buf.delete(&mut start, &mut end);
                }
            }
        }
        if cfg.final_newline && buf.char_count() > 0 {
            let mut end = buf.end_iter();
            let mut prev = end;
            if prev.backward_char() && prev.char() != Some('\n') {
                buf.insert(&mut end, "\n");
            }
        }
        buf.end_user_action();
    }

    pub fn cursor_line_col(&self) -> (i32, i32) {
        let iter = self.buffer.iter_at_mark(&insert_mark(&self.buffer));
        let line = iter.line() + 1;
        // Kolom dihitung dengan memperhitungkan lebar tab.
        let col = self.view.visual_column(&iter) as i32 + 1;
        (line, col)
    }
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn pango_ellipsize_end() -> gtk::pango::EllipsizeMode {
    gtk::pango::EllipsizeMode::End
}

pub fn insert_mark(buf: &sourceview4::Buffer) -> gtk::TextMark {
    buf.get_insert().expect("buffer selalu punya mark insert")
}
