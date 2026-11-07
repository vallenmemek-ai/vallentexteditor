//! Bar cari & ganti di bawah editor (mirip Pluma / gedit).

use crate::document::Document;
use gtk::prelude::*;
use sourceview4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct FindBar {
    pub container: gtk::Box,
    search_entry: gtk::SearchEntry,
    replace_row: gtk::Box,
    replace_entry: gtk::Entry,
    case_check: gtk::CheckButton,
    regex_check: gtk::CheckButton,
    info: gtk::Label,
    settings: sourceview4::SearchSettings,
    context: RefCell<Option<(Rc<Document>, sourceview4::SearchContext)>>,
}

impl FindBar {
    pub fn new() -> Rc<FindBar> {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 4);
        container.set_border_width(4);

        let row1 = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let search_entry = gtk::SearchEntry::new();
        search_entry.set_hexpand(true);
        search_entry.set_placeholder_text(Some("Cari…"));
        let prev = gtk::Button::from_icon_name(Some("go-up-symbolic"), gtk::IconSize::Button);
        let next = gtk::Button::from_icon_name(Some("go-down-symbolic"), gtk::IconSize::Button);
        prev.set_tooltip_text(Some("Sebelumnya (Shift+Ctrl+G)"));
        next.set_tooltip_text(Some("Berikutnya (Ctrl+G)"));
        let case_check = gtk::CheckButton::with_label("Aa");
        case_check.set_tooltip_text(Some("Peka huruf besar/kecil"));
        let regex_check = gtk::CheckButton::with_label("Regex");
        let info = gtk::Label::new(None);
        let close = gtk::Button::from_icon_name(Some("window-close-symbolic"), gtk::IconSize::Button);
        close.set_relief(gtk::ReliefStyle::None);
        for w in [
            search_entry.upcast_ref::<gtk::Widget>(),
            prev.upcast_ref(),
            next.upcast_ref(),
            case_check.upcast_ref(),
            regex_check.upcast_ref(),
            info.upcast_ref(),
        ] {
            row1.pack_start(w, w == search_entry.upcast_ref::<gtk::Widget>(), true, 0);
        }
        row1.pack_end(&close, false, false, 0);

        let replace_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let replace_entry = gtk::Entry::new();
        replace_entry.set_hexpand(true);
        replace_entry.set_placeholder_text(Some("Ganti dengan…"));
        let rep = gtk::Button::with_label("Ganti");
        let rep_all = gtk::Button::with_label("Ganti semua");
        replace_row.pack_start(&replace_entry, true, true, 0);
        replace_row.pack_start(&rep, false, false, 0);
        replace_row.pack_start(&rep_all, false, false, 0);

        container.pack_start(&row1, false, false, 0);
        container.pack_start(&replace_row, false, false, 0);

        let fb = Rc::new(FindBar {
            container,
            search_entry,
            replace_row,
            replace_entry,
            case_check,
            regex_check,
            info,
            settings: sourceview4::SearchSettings::new(),
            context: RefCell::new(None),
        });
        fb.settings.set_wrap_around(true);

        let f = fb.clone();
        fb.search_entry.connect_search_changed(move |_| {
            f.sync_settings();
            f.find_next(true);
        });
        let f = fb.clone();
        fb.search_entry.connect_activate(move |_| f.find_next(false));
        let f = fb.clone();
        next.connect_clicked(move |_| f.find_next(false));
        let f = fb.clone();
        prev.connect_clicked(move |_| f.find_prev());
        let f = fb.clone();
        fb.case_check.connect_toggled(move |_| {
            f.sync_settings();
            f.update_info();
        });
        let f = fb.clone();
        fb.regex_check.connect_toggled(move |_| {
            f.sync_settings();
            f.update_info();
        });
        let f = fb.clone();
        rep.connect_clicked(move |_| f.replace_one());
        let f = fb.clone();
        rep_all.connect_clicked(move |_| f.replace_all());
        let f = fb.clone();
        fb.replace_entry.connect_activate(move |_| f.replace_one());
        let f = fb.clone();
        close.connect_clicked(move |_| f.hide());
        let f = fb.clone();
        fb.search_entry.connect_stop_search(move |_| f.hide());

        fb.container.show_all();
        fb.container.set_no_show_all(true);
        fb.container.hide();
        fb
    }

    fn sync_settings(&self) {
        let text = self.search_entry.text();
        self.settings.set_search_text(if text.is_empty() { None } else { Some(text.as_str()) });
        self.settings.set_case_sensitive(self.case_check.is_active());
        self.settings.set_regex_enabled(self.regex_check.is_active());
    }

    /// Dipanggil saat tab aktif berganti.
    pub fn attach(&self, doc: Option<Rc<Document>>) {
        if let Some((old, ctx)) = self.context.borrow_mut().take() {
            ctx.set_highlight(false);
            drop(old);
        }
        if let Some(d) = doc {
            let ctx = sourceview4::SearchContext::new(&d.buffer, Some(&self.settings));
            ctx.set_highlight(self.container.is_visible());
            *self.context.borrow_mut() = Some((d, ctx));
            self.update_info();
        }
    }

    pub fn show(&self, replace: bool, selection: Option<String>) {
        self.container.set_no_show_all(false);
        self.container.show_all();
        self.container.set_no_show_all(true);
        if replace {
            self.replace_row.show();
        } else {
            self.replace_row.hide();
        }
        if let Some(s) = selection.filter(|s| !s.is_empty() && !s.contains('\n')) {
            self.search_entry.set_text(&s);
        }
        self.sync_settings();
        if let Some((_, ctx)) = &*self.context.borrow() {
            ctx.set_highlight(true);
        }
        self.search_entry.grab_focus();
        self.update_info();
    }

    pub fn hide(&self) {
        self.container.hide();
        if let Some((d, ctx)) = &*self.context.borrow() {
            ctx.set_highlight(false);
            d.view.grab_focus();
        }
    }

    fn update_info(&self) {
        let ctx = self.context.borrow();
        let Some((_, ctx)) = &*ctx else { return };
        if self.search_entry.text().is_empty() {
            self.info.set_text("");
            return;
        }
        let n = ctx.occurrences_count();
        self.info.set_text(&if n < 0 { "…".to_string() } else { format!("{n} cocok") });
    }

    /// `from_start_of_selection`: dipakai saat mengetik, supaya hasil pertama
    /// tidak lompat melewati kecocokan di posisi kursor.
    pub fn find_next(&self, from_start_of_selection: bool) {
        let ctx = self.context.borrow();
        let Some((doc, ctx)) = &*ctx else { return };
        if self.search_entry.text().is_empty() {
            return;
        }
        let buf = &doc.buffer;
        let iter = match buf.selection_bounds() {
            Some((s, e)) => {
                if from_start_of_selection { s } else { e }
            }
            None => buf.iter_at_mark(&crate::document::insert_mark(buf)),
        };
        match ctx.forward(&iter) {
            Some((s, e, _)) => {
                buf.select_range(&s, &e);
                let mut s2 = s;
                doc.view.scroll_to_iter(&mut s2, 0.1, false, 0.0, 0.0);
                self.info.set_tooltip_text(None);
            }
            None => self.info.set_text("Tidak ditemukan"),
        }
        self.update_info_keep();
    }

    pub fn find_prev(&self) {
        let ctx = self.context.borrow();
        let Some((doc, ctx)) = &*ctx else { return };
        if self.search_entry.text().is_empty() {
            return;
        }
        let buf = &doc.buffer;
        let iter = match buf.selection_bounds() {
            Some((s, _)) => s,
            None => buf.iter_at_mark(&crate::document::insert_mark(buf)),
        };
        match ctx.backward(&iter) {
            Some((s, e, _)) => {
                buf.select_range(&s, &e);
                let mut s2 = s;
                doc.view.scroll_to_iter(&mut s2, 0.1, false, 0.0, 0.0);
            }
            None => self.info.set_text("Tidak ditemukan"),
        }
    }

    fn update_info_keep(&self) {
        // Hanya perbarui hitungan kalau ada hasil, supaya "Tidak ditemukan" tidak tertimpa.
        let ctx = self.context.borrow();
        if let Some((_, ctx)) = &*ctx {
            let n = ctx.occurrences_count();
            if n > 0 {
                self.info.set_text(&format!("{n} cocok"));
            }
        }
    }

    fn replace_one(&self) {
        let ctx = self.context.borrow();
        let Some((doc, ctx)) = &*ctx else { return };
        let with = self.replace_entry.text();
        if let Some((mut s, mut e)) = doc.buffer.selection_bounds() {
            // Hanya ganti kalau seleksi memang sebuah kecocokan.
            if ctx.forward(&s).map(|(ms, me, _)| ms == s && me == e).unwrap_or(false) {
                let _ = ctx.replace(&mut s, &mut e, with.as_str());
            }
        }
        self.find_next(false);
    }

    fn replace_all(&self) {
        let ctx = self.context.borrow();
        let Some((_, ctx)) = &*ctx else { return };
        let n = ctx.occurrences_count().max(0);
        let with = self.replace_entry.text();
        let _ = ctx.replace_all(with.as_str());
        self.info.set_text(&format!("{n} diganti"));
    }
}
