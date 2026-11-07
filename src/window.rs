//! Jendela utama: menu, tab, status bar, aksi-aksi editor.

use crate::config::Config;
use crate::document::Document;
use crate::file_io::Encoding;
use crate::findbar::FindBar;
use crate::session;
use gtk::prelude::*;
use gtk::traits::SettingsExt;
use sourceview4::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

/// Popover pilih bahasa — dibangun malas (saat pertama diklik), bukan saat start.
struct LangPicker {
    popover: gtk::Popover,
    search: gtk::SearchEntry,
}

pub struct VallenTextWindow {
    pub window: gtk::ApplicationWindow,
    notebook: gtk::Notebook,
    docs: RefCell<Vec<Rc<Document>>>,
    vbox: gtk::Box,
    findbar: RefCell<Option<Rc<FindBar>>>,
    status_box: gtk::Box,
    status_pos: gtk::Label,
    status_info: gtk::Label,
    lang_button: gtk::Button,
    lang_picker: RefCell<Option<LangPicker>>,
    status_pending: Cell<bool>,
    config: RefCell<Config>,
    css: gtk::CssProvider,
    untitled_counter: Cell<u32>,
    recent_menu: gio::Menu,
    next_rid: Cell<u32>,
    checking_disk: Cell<bool>,
}

impl VallenTextWindow {
    pub fn new(app: &gtk::Application) -> Rc<VallenTextWindow> {
        let config = Config::load();

        let window = gtk::ApplicationWindow::new(app);
        window.set_title("VallenText");
        window.set_default_size(config.win_w, config.win_h);
        if config.maximized {
            window.maximize();
        }

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let recent_menu = gio::Menu::new();
        let menubar = gtk::MenuBar::from_model(&build_menu(&recent_menu));
        vbox.pack_start(&menubar, false, false, 0);

        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_show_border(false);
        vbox.pack_start(&notebook, true, true, 0);

        let status_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        status_box.set_border_width(3);
        let status_pos = gtk::Label::new(Some("Ln 1, Kol 1"));
        let status_info = gtk::Label::new(None);
        let lang_button = gtk::Button::with_label("Teks biasa");
        lang_button.set_relief(gtk::ReliefStyle::None);
        lang_button.set_focus_on_click(false);
        lang_button.set_tooltip_text(Some("Pilih bahasa sintaks"));
        status_box.pack_start(&status_pos, false, false, 6);
        status_box.pack_end(&lang_button, false, false, 0);
        status_box.pack_end(&status_info, false, false, 6);
        vbox.pack_end(&status_box, false, false, 0);
        window.add(&vbox);

        let css = gtk::CssProvider::new();
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(&screen, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }

        let win = Rc::new(VallenTextWindow {
            window,
            notebook,
            docs: RefCell::new(Vec::new()),
            vbox,
            findbar: RefCell::new(None),
            status_box,
            status_pos,
            status_info,
            lang_button,
            lang_picker: RefCell::new(None),
            status_pending: Cell::new(false),
            config: RefCell::new(config),
            css,
            untitled_counter: Cell::new(0),
            recent_menu,
            next_rid: Cell::new(1),
            checking_disk: Cell::new(false),
        });

        {
            let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
            let h2 = handler.clone();
            let id = win.window.connect_draw(move |w, _| {
                crate::trace::mark("frame pertama tergambar");
                if let Some(id) = h2.borrow_mut().take() {
                    w.disconnect(id);
                }
                gtk::Inhibit(false)
            });
            *handler.borrow_mut() = Some(id);
        }

        win.apply_global_config();
        win.setup_actions();
        win.setup_signals();
        win.window.show_all();
        win.status_box.set_visible(win.config.borrow().statusbar);
        {
            let w = win.clone();
            glib::idle_add_local_once(move || w.rebuild_recent());
        }
        win.start_autosave();
        win
    }

    // ───────────────────────── pengaturan ─────────────────────────

    fn apply_global_config(&self) {
        let cfg = self.config.borrow().clone();
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_application_prefer_dark_theme(cfg.dark);
            settings.set_gtk_enable_animations(false);
        }
        let css = format!("textview {{ font-family: monospace; font-size: {}pt; }}", cfg.font_size);
        let _ = self.css.load_from_data(css.as_bytes());
        self.status_box.set_visible(cfg.statusbar);
        for d in self.docs.borrow().iter() {
            d.apply_config(&cfg);
        }
    }

    fn with_config(&self, f: impl FnOnce(&mut Config)) {
        f(&mut self.config.borrow_mut());
        self.apply_global_config();
    }

    /// Bar cari & ganti dibangun saat pertama dibutuhkan (Ctrl+F), bukan saat start.
    fn findbar(self: &Rc<Self>) -> Rc<FindBar> {
        if let Some(f) = self.findbar.borrow().clone() {
            return f;
        }
        let f = FindBar::new();
        self.vbox.pack_start(&f.container, false, false, 0);
        f.attach(self.current());
        *self.findbar.borrow_mut() = Some(f.clone());
        f
    }

    fn findbar_if_ready(&self) -> Option<Rc<FindBar>> {
        self.findbar.borrow().clone()
    }

    // ───────────────────────── dokumen / tab ─────────────────────────

    pub fn current(&self) -> Option<Rc<Document>> {
        let page = self.notebook.current_page()?;
        let widget = self.notebook.nth_page(Some(page))?;
        self.docs.borrow().iter().find(|d| d.scroller.upcast_ref::<gtk::Widget>() == &widget).cloned()
    }

    pub fn doc_count(&self) -> usize {
        self.docs.borrow().len()
    }

    pub fn new_doc(self: &Rc<Self>) -> Rc<Document> {
        let n = self.untitled_counter.get() + 1;
        self.untitled_counter.set(n);
        let doc = Document::new(format!("Dokumen Belum Disimpan {n}"), &self.config.borrow());
        self.add_doc(&doc);
        doc
    }

    fn add_doc(self: &Rc<Self>, doc: &Rc<Document>) {
        // Label tab + tombol tutup, klik tengah juga menutup.
        let tab = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tab.pack_start(&doc.tab_label, true, true, 0);
        let close = gtk::Button::from_icon_name(Some("window-close-symbolic"), gtk::IconSize::Menu);
        close.set_relief(gtk::ReliefStyle::None);
        close.set_focus_on_click(false);
        tab.pack_start(&close, false, false, 0);
        let ev = gtk::EventBox::new();
        ev.add(&tab);
        ev.show_all();

        let w = Rc::downgrade(self);
        let d = Rc::downgrade(doc);
        close.connect_clicked(move |_| {
            if let (Some(w), Some(d)) = (w.upgrade(), d.upgrade()) {
                w.close_doc(&d);
            }
        });
        let w = Rc::downgrade(self);
        let d = Rc::downgrade(doc);
        ev.connect_button_press_event(move |_, e| {
            if e.button() == 2 {
                if let (Some(w), Some(d)) = (w.upgrade(), d.upgrade()) {
                    w.close_doc(&d);
                }
                return gtk::Inhibit(true);
            }
            gtk::Inhibit(false)
        });

        if doc.rid.get() == 0 {
            doc.rid.set(self.next_rid.get());
            self.next_rid.set(self.next_rid.get() + 1);
        }
        self.docs.borrow_mut().push(doc.clone());
        let idx = self.notebook.append_page(&doc.scroller, Some(&ev));
        self.notebook.set_tab_reorderable(&doc.scroller, true);
        doc.scroller.show_all();
        self.notebook.set_current_page(Some(idx));

        let w: Weak<VallenTextWindow> = Rc::downgrade(self);
        doc.buffer.connect_modified_changed(move |_| {
            if let Some(w) = w.upgrade() {
                w.refresh_titles();
            }
        });
        let w = Rc::downgrade(self);
        let d = Rc::downgrade(doc);
        doc.buffer.connect_mark_set(move |_, _, mark| {
            if matches!(mark.name().as_deref(), Some("insert") | Some("selection_bound")) {
                if let (Some(w), Some(d)) = (w.upgrade(), d.upgrade()) {
                    if w.current().map(|c| Rc::ptr_eq(&c, &d)).unwrap_or(false) {
                        w.schedule_status();
                    }
                }
            }
        });
        let w = Rc::downgrade(self);
        doc.buffer.connect_changed(move |_| {
            if let Some(w) = w.upgrade() {
                w.schedule_status();
            }
        });

        let d = Rc::downgrade(doc);
        doc.buffer.connect_changed(move |_| {
            if let Some(d) = d.upgrade() {
                d.dirty.set(true);
            }
        });

        doc.view.grab_focus();
        self.refresh_titles();
        self.refresh_status();
        if let Some(f) = self.findbar_if_ready() {
            f.attach(Some(doc.clone()));
        }
    }

    pub fn open_files(self: &Rc<Self>, paths: &[PathBuf]) {
        for p in paths {
            let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
            // Sudah terbuka? Pindah ke tab itu saja.
            let existing = self
                .docs
                .borrow()
                .iter()
                .find(|d| d.path.borrow().as_deref() == Some(canon.as_path()))
                .cloned();
            if let Some(d) = existing {
                if let Some(n) = self.notebook.page_num(&d.scroller) {
                    self.notebook.set_current_page(Some(n));
                }
                continue;
            }
            // Tab kosong yang belum dipakai langsung dipakai ulang.
            let reuse = self.current().filter(|d| d.is_pristine_untitled());
            let doc = match reuse {
                Some(d) => d,
                None => {
                    let n = self.untitled_counter.get() + 1;
                    self.untitled_counter.set(n);
                    Document::new(format!("Dokumen Belum Disimpan {n}"), &self.config.borrow())
                }
            };
            let is_new = !self.docs.borrow().iter().any(|d| Rc::ptr_eq(d, &doc));
            if !p.exists() {
                // File belum ada → buat dokumen kosong dengan nama itu (perilaku seperti Pluma).
                *doc.path.borrow_mut() = Some(canon.clone());
                doc.set_language_by_path(&canon);
            } else {
                if let Ok(m) = std::fs::metadata(&canon) {
                    if m.len() > 20 * 1024 * 1024
                        && !self.ask(
                            &format!("{} berukuran {} MB", p.display(), m.len() / (1024 * 1024)),
                            "File besar bisa membuat editor lambat dan memakai banyak RAM. Tetap dibuka?",
                            "Buka",
                            "Batal",
                        )
                    {
                        continue;
                    }
                }
                let t0 = std::time::Instant::now();
                if let Err(e) = doc.load(&canon) {
                    self.error_dialog(&format!("Tidak bisa membuka {}", p.display()), &e);
                    continue;
                }
                crate::trace::mark(&format!(
                    "file dimuat: {} ({} KB, {:.1} ms)",
                    p.display(),
                    doc.buffer.char_count() / 1024,
                    t0.elapsed().as_secs_f64() * 1000.0
                ));
                {
                    // Pasang bahasa sintaks & catat "terbaru" setelah teks sempat tampil.
                    let (d2, w, pc) = (doc.clone(), self.clone(), canon.clone());
                    glib::idle_add_local_once(move || {
                        d2.set_language_by_path(&pc);
                        w.push_recent(&pc);
                        w.schedule_status();
                    });
                }
            }
            if is_new {
                self.add_doc(&doc);
            } else {
                self.refresh_titles();
                self.refresh_status();
            }
        }
    }

    pub fn close_doc(self: &Rc<Self>, doc: &Rc<Document>) -> bool {
        if doc.is_modified() {
            if let Some(n) = self.notebook.page_num(&doc.scroller) {
                self.notebook.set_current_page(Some(n));
            }
            let dialog = gtk::MessageDialog::new(
                Some(&self.window),
                gtk::DialogFlags::MODAL,
                gtk::MessageType::Warning,
                gtk::ButtonsType::None,
                &format!("Simpan perubahan pada “{}” sebelum menutup?", doc.display_name()),
            );
            dialog.set_secondary_text(Some("Kalau tidak disimpan, perubahan akan hilang."));
            dialog.add_button("Tutup tanpa menyimpan", gtk::ResponseType::Reject);
            dialog.add_button("Batal", gtk::ResponseType::Cancel);
            dialog.add_button("Simpan", gtk::ResponseType::Accept);
            dialog.set_default_response(gtk::ResponseType::Accept);
            let resp = dialog.run();
            dialog.close();
            match resp {
                gtk::ResponseType::Accept => {
                    if !self.save_doc(doc) {
                        return false;
                    }
                }
                gtk::ResponseType::Reject => {}
                _ => return false,
            }
        }
        session::remove_recovery(doc.rid.get());
        if let Some(n) = self.notebook.page_num(&doc.scroller) {
            self.notebook.remove_page(Some(n));
        }
        self.docs.borrow_mut().retain(|d| !Rc::ptr_eq(d, doc));
        if self.docs.borrow().is_empty() {
            self.new_doc();
        }
        true
    }

    /// Dipanggil saat jendela ditutup. true = boleh keluar.
    fn confirm_quit(self: &Rc<Self>) -> bool {
        let docs: Vec<_> = self.docs.borrow().clone();
        for d in docs {
            if d.is_modified() && !self.close_doc(&d) {
                return false;
            }
        }
        {
            let docs = self.docs.borrow();
            let paths: Vec<PathBuf> = docs.iter().filter_map(|d| d.path.borrow().clone()).collect();
            let cur = self
                .current()
                .and_then(|c| c.path.borrow().clone())
                .and_then(|p| paths.iter().position(|x| *x == p))
                .unwrap_or(0);
            session::save_session(&paths, cur);
        }
        let mut cfg = self.config.borrow_mut();
        cfg.maximized = self.window.is_maximized();
        if !cfg.maximized {
            let (w, h) = self.window.size();
            cfg.win_w = w;
            cfg.win_h = h;
        }
        cfg.save();
        true
    }

    // ───────────────────────── simpan / dialog ─────────────────────────

    fn save_doc(self: &Rc<Self>, doc: &Rc<Document>) -> bool {
        let path = doc.path.borrow().clone();
        let cfg = self.config.borrow().clone();
        match path {
            Some(p) => match doc.save_to(&p, &cfg) {
                Ok(()) => {
                    self.after_save(doc, &p);
                    true
                }
                Err(e) => {
                    self.error_dialog(&format!("Gagal menyimpan {}", p.display()), &e);
                    false
                }
            },
            None => self.save_doc_as(doc),
        }
    }

    fn save_doc_as(self: &Rc<Self>, doc: &Rc<Document>) -> bool {
        let chooser = gtk::FileChooserNative::new(
            Some("Simpan Sebagai"),
            Some(&self.window),
            gtk::FileChooserAction::Save,
            Some("Simpan"),
            Some("Batal"),
        );
        chooser.set_do_overwrite_confirmation(true);
        match doc.path.borrow().as_ref() {
            Some(p) => {
                let _ = chooser.set_filename(p);
            }
            None => chooser.set_current_name("tanpa-judul.txt"),
        }
        if chooser.run() != gtk::ResponseType::Accept {
            return false;
        }
        let Some(path) = chooser.filename() else { return false };
        let cfg = self.config.borrow().clone();
        match doc.save_to(&path, &cfg) {
            Ok(()) => {
                self.after_save(doc, &path);
                true
            }
            Err(e) => {
                self.error_dialog(&format!("Gagal menyimpan {}", path.display()), &e);
                false
            }
        }
    }

    fn open_dialog(self: &Rc<Self>) {
        let chooser = gtk::FileChooserNative::new(
            Some("Buka File"),
            Some(&self.window),
            gtk::FileChooserAction::Open,
            Some("Buka"),
            Some("Batal"),
        );
        chooser.set_select_multiple(true);
        if let Some(dir) = self.current().and_then(|d| d.path.borrow().as_ref().and_then(|p| p.parent().map(Path::to_path_buf))) {
            let _ = chooser.set_current_folder(dir);
        }
        if chooser.run() == gtk::ResponseType::Accept {
            self.open_files(&chooser.filenames());
        }
    }

    fn error_dialog(&self, title: &str, detail: &str) {
        let d = gtk::MessageDialog::new(
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Error,
            gtk::ButtonsType::Close,
            title,
        );
        d.set_secondary_text(Some(detail));
        d.run();
        d.close();
    }

    fn goto_line_dialog(&self) {
        let Some(doc) = self.current() else { return };
        let total = doc.buffer.line_count().max(1);
        let dialog = gtk::Dialog::with_buttons(
            Some("Ke Baris"),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[("Batal", gtk::ResponseType::Cancel), ("Pergi", gtk::ResponseType::Accept)],
        );
        dialog.set_default_response(gtk::ResponseType::Accept);
        let spin = gtk::SpinButton::with_range(1.0, total as f64, 1.0);
        spin.set_activates_default(true);
        spin.set_value((doc.cursor_line_col().0) as f64);
        dialog.content_area().set_border_width(12);
        dialog.content_area().add(&spin);
        dialog.show_all();
        if dialog.run() == gtk::ResponseType::Accept {
            let line = spin.value_as_int() - 1;
            let iter = doc.buffer.iter_at_line(line);
            doc.buffer.place_cursor(&iter);
            let mark = crate::document::insert_mark(&doc.buffer);
            doc.view.scroll_to_mark(&mark, 0.1, true, 0.5, 0.5);
        }
        dialog.close();
        doc.view.grab_focus();
    }

    fn about_dialog(&self) {
        let d = gtk::AboutDialog::new();
        d.set_transient_for(Some(&self.window));
        d.set_program_name("VallenText");
        d.set_version(Some(env!("CARGO_PKG_VERSION")));
        d.set_comments(Some("Editor teks ringan ala Pluma, ditulis dengan Rust + GTK3."));
        match crate::app::icon_pixbuf() {
            Some(pb) => d.set_logo(Some(&pb)),
            None => d.set_logo_icon_name(Some("vallentext")),
        }
        d.set_license_type(gtk::License::MitX11);
        d.run();
        d.close();
    }

    // ───────────────────────── judul & status ─────────────────────────

    fn refresh_titles(&self) {
        for d in self.docs.borrow().iter() {
            let mark = if d.is_modified() { "*" } else { "" };
            d.tab_label.set_text(&format!("{mark}{}", d.display_name()));
            let tip = d.path.borrow().as_ref().map(|p| p.display().to_string());
            d.tab_label.set_tooltip_text(tip.as_deref());
        }
        match self.current() {
            Some(d) => {
                let mark = if d.is_modified() { "*" } else { "" };
                let dir = d
                    .path
                    .borrow()
                    .as_ref()
                    .and_then(|p| p.parent().map(|p| p.display().to_string()))
                    .map(|p| format!(" ({p})"))
                    .unwrap_or_default();
                self.window.set_title(&format!("{mark}{}{dir} - VallenText", d.display_name()));
            }
            None => self.window.set_title("VallenText"),
        }
    }

    fn refresh_status(&self) {
        let Some(d) = self.current() else { return };
        let (l, c) = d.cursor_line_col();
        let mut pos = format!("Ln {l}, Kol {c}");
        if let Some((a, b)) = d.buffer.selection_bounds() {
            let n = b.offset() - a.offset();
            let lines = b.line() - a.line() + 1;
            pos.push_str(&format!("  ·  {n} karakter terpilih"));
            if lines > 1 {
                pos.push_str(&format!(" ({lines} baris)"));
            }
        }
        self.status_pos.set_text(&pos);
        let eol = if d.crlf.get() { "CRLF" } else { "LF" };
        let star = if d.mixed_eol.get() { "*" } else { "" };
        self.status_info.set_text(&format!(
            "{} baris  ·  {eol}{star}  ·  {}",
            d.buffer.line_count(),
            d.encoding.get().label()
        ));
        self.status_info.set_tooltip_text(if d.mixed_eol.get() {
            Some("Akhir baris file ini campuran (CRLF dan LF); akan diseragamkan saat disimpan")
        } else {
            None
        });
        let lang = d
            .buffer
            .language()
            .and_then(|l| l.name())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "Teks biasa".to_string());
        if self.lang_button.label().as_deref() != Some(lang.as_str()) {
            self.lang_button.set_label(&lang);
        }
    }

    /// Gabungkan banyak update status (tiap ketikan/paste) jadi satu per siklus idle.
    fn schedule_status(self: &Rc<Self>) {
        if self.status_pending.replace(true) {
            return;
        }
        let w = self.clone();
        glib::idle_add_local_once(move || {
            w.status_pending.set(false);
            w.refresh_status();
        });
    }

    fn set_current_language(&self, id: &str) {
        if let Some(d) = self.current() {
            let lang = if id == "-" {
                None
            } else {
                sourceview4::LanguageManager::default().and_then(|lm| lm.language(id))
            };
            d.buffer.set_language(lang.as_ref());
            d.view.grab_focus();
        }
        self.refresh_status();
    }

    fn show_lang_picker(self: &Rc<Self>) {
        if self.lang_picker.borrow().is_none() {
            let popover = gtk::Popover::new(Some(&self.lang_button));
            let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
            vbox.set_border_width(6);
            let search = gtk::SearchEntry::new();
            search.set_placeholder_text(Some("Cari bahasa…"));
            let scroller = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
            scroller.set_min_content_height(320);
            scroller.set_min_content_width(240);
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::None);

            let lm = sourceview4::LanguageManager::default().expect("LanguageManager");
            let mut langs: Vec<(String, String)> = lm
                .language_ids()
                .iter()
                .filter_map(|id| lm.language(id).map(|l| (l.name().map(|n| n.to_string()).unwrap_or_default(), id.to_string())))
                .collect();
            langs.sort_by_key(|(n, _)| n.to_lowercase());
            langs.insert(0, ("Teks biasa".to_string(), "-".to_string()));
            for (name, id) in &langs {
                let row = gtk::ListBoxRow::new();
                row.set_widget_name(id); // id bahasa disimpan di nama widget
                let label = gtk::Label::new(Some(name));
                label.set_xalign(0.0);
                label.set_margin_start(6);
                label.set_margin_end(6);
                row.add(&label);
                list.add(&row);
            }
            scroller.add(&list);
            vbox.pack_start(&search, false, false, 0);
            vbox.pack_start(&scroller, true, true, 0);
            vbox.show_all();
            popover.add(&vbox);

            let s = search.clone();
            list.set_filter_func(Some(Box::new(move |row| {
                let q = s.text().to_lowercase();
                if q.is_empty() {
                    return true;
                }
                row.child()
                    .and_then(|c| c.downcast::<gtk::Label>().ok())
                    .map(|l| l.text().to_lowercase().contains(&q))
                    .unwrap_or(true)
            })));
            let l2 = list.clone();
            search.connect_search_changed(move |_| l2.invalidate_filter());
            let w = Rc::downgrade(self);
            let pop = popover.clone();
            list.connect_row_activated(move |_, row| {
                pop.hide();
                if let Some(w) = w.upgrade() {
                    w.set_current_language(row.widget_name().as_str());
                }
            });
            *self.lang_picker.borrow_mut() = Some(LangPicker { popover, search });
        }
        if let Some(p) = &*self.lang_picker.borrow() {
            p.search.set_text("");
            p.popover.show_all();
            p.search.grab_focus();
        }
    }

    // ───────────────────────── fitur tambahan ─────────────────────────

    /// Dialog ya/tidak sederhana.
    fn ask(&self, title: &str, detail: &str, yes: &str, no: &str) -> bool {
        let d = gtk::MessageDialog::new(
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Question,
            gtk::ButtonsType::None,
            title,
        );
        d.set_secondary_text(Some(detail));
        d.add_button(no, gtk::ResponseType::Cancel);
        d.add_button(yes, gtk::ResponseType::Accept);
        d.set_default_response(gtk::ResponseType::Accept);
        let ok = d.run() == gtk::ResponseType::Accept;
        d.close();
        ok
    }

    fn after_save(&self, doc: &Rc<Document>, path: &Path) {
        session::remove_recovery(doc.rid.get());
        self.push_recent(path);
        self.refresh_titles();
        self.refresh_status();
    }

    fn push_recent(&self, path: &Path) {
        session::push_recent(path);
        self.rebuild_recent();
    }

    fn rebuild_recent(&self) {
        self.recent_menu.remove_all();
        let items = session::load_recent();
        if items.is_empty() {
            self.recent_menu.append(Some("(kosong)"), None);
            return;
        }
        for p in &items {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
            // "_" di label menu berarti mnemonic, jadi digandakan.
            let label = format!("{name}  —  {dir}").replace('_', "__");
            let item = gio::MenuItem::new(Some(&label), None);
            let target = p.to_string_lossy().into_owned().to_variant();
            item.set_action_and_target_value(Some("win.open-recent"), Some(&target));
            self.recent_menu.append_item(&item);
        }
        let clear = gio::Menu::new();
        clear.append(Some("Bersihkan Daftar"), Some("win.clear-recent"));
        self.recent_menu.append_section(None, &clear);
    }

    /// Autosave tiap 30 detik untuk dokumen yang belum disimpan (pemulihan kalau crash/mati listrik).
    fn start_autosave(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        glib::timeout_add_seconds_local(30, move || match w.upgrade() {
            Some(w) => {
                w.autosave_tick();
                glib::Continue(true)
            }
            None => glib::Continue(false),
        });
    }

    fn autosave_tick(&self) {
        for d in self.docs.borrow().iter() {
            if !d.dirty.get() {
                continue;
            }
            if d.is_modified() {
                if d.buffer.char_count() > 5_000_000 {
                    continue; // terlalu besar: jangan bikin macet tiap 30 detik
                }
                let (a, b) = d.buffer.bounds();
                if let Some(t) = d.buffer.text(&a, &b, false) {
                    session::write_recovery(d.rid.get(), d.path.borrow().as_deref(), t.as_str());
                    d.dirty.set(false);
                }
            } else {
                session::remove_recovery(d.rid.get());
                d.dirty.set(false);
            }
        }
    }

    /// Dipanggil saat start tanpa argumen: pulihkan dokumen sisa crash, lalu buka tab sesi terakhir.
    pub fn restore_session(self: &Rc<Self>) {
        let recs = session::load_recoveries();
        let max = recs.iter().map(|r| r.id).max().unwrap_or(0);
        if self.next_rid.get() <= max {
            self.next_rid.set(max + 1);
        }
        for r in recs {
            let n = self.untitled_counter.get() + 1;
            self.untitled_counter.set(n);
            let doc = Document::new(format!("Dokumen Belum Disimpan {n}"), &self.config.borrow());
            doc.restore(r.path, &r.text);
            doc.rid.set(r.id);
            self.add_doc(&doc);
        }
        crate::trace::mark("dokumen pemulihan dibuka");
        let (paths, cur) = session::load_session();
        let mut existing: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists()).collect();
        if cur < existing.len() {
            let c = existing.remove(cur);
            existing.insert(0, c); // tab aktif dibuka duluan
        }
        self.open_staggered(existing, true);
    }

    /// Buka file sesi satu per siklus idle supaya jendela tetap responsif walau tabnya banyak.
    /// Tab yang sedang aktif tidak direbut oleh file yang baru selesai dimuat.
    fn open_staggered(self: &Rc<Self>, mut rest: Vec<PathBuf>, is_first: bool) {
        if rest.is_empty() {
            self.drop_extra_blank();
            crate::trace::mark("semua tab sesi terbuka");
            return;
        }
        let p = rest.remove(0);
        let before = self.notebook.current_page();
        self.open_files(std::slice::from_ref(&p));
        if !is_first {
            if let Some(b) = before {
                self.notebook.set_current_page(Some(b));
            }
        }
        let w = self.clone();
        glib::idle_add_local_once(move || w.open_staggered(rest, false));
    }

    /// Tutup tab kosong bawaan kalau sudah ada tab lain (dan belum diketik apa-apa).
    fn drop_extra_blank(self: &Rc<Self>) {
        let extra: Vec<Rc<Document>> =
            self.docs.borrow().iter().filter(|d| d.is_pristine_untitled()).cloned().collect();
        for d in extra {
            if self.doc_count() > 1 {
                self.close_doc(&d);
            }
        }
    }

    /// Cek apakah file tab aktif berubah di disk oleh program lain.
    fn check_disk_changes(self: &Rc<Self>) {
        if self.checking_disk.replace(true) {
            return;
        }
        if let Some(doc) = self.current() {
            let path = doc.path.borrow().clone();
            if let (Some(p), Some(old)) = (path, doc.mtime.get()) {
                if let Ok(now) = std::fs::metadata(&p).and_then(|m| m.modified()) {
                    if now != old {
                        doc.mtime.set(Some(now)); // jangan tanya berulang-ulang
                        let detail = if doc.is_modified() {
                            "Kamu juga punya perubahan yang belum disimpan; memuat ulang akan membuangnya."
                        } else {
                            "File diubah oleh program lain."
                        };
                        if self.ask(
                            &format!("“{}” berubah di disk", doc.display_name()),
                            detail,
                            "Muat Ulang",
                            "Biarkan",
                        ) {
                            self.reload_current(None);
                        }
                    }
                }
            }
        }
        self.checking_disk.set(false);
    }

    /// Muat ulang tab aktif dari disk (opsional dengan encoding tertentu); posisi baris dipertahankan.
    fn reload_current(self: &Rc<Self>, forced: Option<Encoding>) {
        let Some(doc) = self.current() else { return };
        let path = doc.path.borrow().clone();
        let Some(path) = path else {
            self.error_dialog("Tidak bisa memuat ulang", "Dokumen ini belum pernah disimpan ke file.");
            return;
        };
        let line = doc.cursor_line_col().0 - 1;
        match doc.load_as(&path, forced) {
            Ok(()) => {
                doc.set_language_by_path(&path);
                let last = (doc.buffer.line_count() - 1).max(0);
                let it = doc.buffer.iter_at_line(line.min(last));
                doc.buffer.place_cursor(&it);
                let mark = crate::document::insert_mark(&doc.buffer);
                doc.view.scroll_to_mark(&mark, 0.1, true, 0.5, 0.5);
                session::remove_recovery(doc.rid.get());
                self.refresh_titles();
                self.refresh_status();
            }
            Err(e) => self.error_dialog(&format!("Tidak bisa memuat ulang {}", path.display()), &e),
        }
    }

    fn reload_action(self: &Rc<Self>, forced: Option<Encoding>) {
        let Some(doc) = self.current() else { return };
        if doc.is_modified()
            && !self.ask(
                "Buang perubahan yang belum disimpan?",
                "Memuat ulang akan mengganti isi dengan versi di disk.",
                "Muat Ulang",
                "Batal",
            )
        {
            return;
        }
        self.reload_current(forced);
    }

    /// Ganti encoding yang dipakai saat menyimpan (isi teks tidak berubah).
    fn set_save_encoding(&self, enc: Encoding) {
        if let Some(d) = self.current() {
            if d.encoding.get() != enc {
                d.encoding.set(enc);
                d.buffer.set_modified(true);
            }
        }
        self.refresh_titles();
        self.refresh_status();
    }

    fn goto_tab(&self, n: u32) {
        if n >= 1 && n <= self.notebook.n_pages() {
            self.notebook.set_current_page(Some(n - 1));
        }
    }

    // ───────────────────────── sinyal ─────────────────────────

    fn setup_signals(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        self.notebook.connect_switch_page(move |_, _, _| {
            // switch-page dikirim sebelum halaman benar-benar aktif; tunda ke idle.
            if let Some(w) = w.upgrade() {
                glib::idle_add_local_once(move || {
                    w.refresh_titles();
                    w.refresh_status();
                    if let Some(f) = w.findbar_if_ready() {
                        f.attach(w.current());
                    }
                });
            }
        });

        let w = Rc::downgrade(self);
        self.lang_button.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.show_lang_picker();
            }
        });

        let w = Rc::downgrade(self);
        self.window.connect_focus_in_event(move |_, _| {
            if let Some(w) = w.upgrade() {
                glib::idle_add_local_once(move || w.check_disk_changes());
            }
            gtk::Inhibit(false)
        });

        let w = self.clone();
        self.window.connect_delete_event(move |_, _| gtk::Inhibit(!w.confirm_quit()));

        // Seret & lepas file ke jendela untuk membukanya.
        self.window.drag_dest_add_uri_targets();
        let w = Rc::downgrade(self);
        self.window.connect_drag_data_received(move |_, _, _, _, data, _, _| {
            if let Some(w) = w.upgrade() {
                let paths: Vec<PathBuf> = data
                    .uris()
                    .iter()
                    .filter_map(|u| gio::File::for_uri(u).path())
                    .collect();
                if !paths.is_empty() {
                    w.open_files(&paths);
                }
            }
        });

        // Ctrl + scroll mouse = zoom.
        let w = Rc::downgrade(self);
        self.window.connect_scroll_event(move |_, e| {
            if e.state().contains(gdk::ModifierType::CONTROL_MASK) {
                if let Some(w) = w.upgrade() {
                    match e.direction() {
                        gdk::ScrollDirection::Up => w.zoom(1),
                        gdk::ScrollDirection::Down => w.zoom(-1),
                        _ => {}
                    }
                    return gtk::Inhibit(true);
                }
            }
            gtk::Inhibit(false)
        });
    }

    fn zoom(&self, delta: i32) {
        self.with_config(|c| c.font_size = (c.font_size as i32 + delta).clamp(6, 48) as u32);
    }

    // ───────────────────────── aksi ─────────────────────────

    fn focused_editable(&self) -> Option<gtk::Editable> {
        self.window.focused_widget().and_then(|w| w.dynamic_cast::<gtk::Editable>().ok())
    }

    fn setup_actions(self: &Rc<Self>) {
        // Aksi biasa.
        macro_rules! act {
            ($name:expr, |$w:ident| $body:block) => {{
                let action = gio::SimpleAction::new($name, None);
                let weak = Rc::downgrade(self);
                action.connect_activate(move |_, _| {
                    if let Some($w) = weak.upgrade() {
                        $body
                    }
                });
                self.window.add_action(&action);
            }};
        }

        act!("new", |w| { w.new_doc(); });
        act!("open", |w| { w.open_dialog(); });
        act!("save", |w| { if let Some(d) = w.current() { w.save_doc(&d); } });
        act!("save-as", |w| { if let Some(d) = w.current() { w.save_doc_as(&d); } });
        act!("save-all", |w| {
            let docs: Vec<_> = w.docs.borrow().clone();
            for d in docs.iter().filter(|d| d.is_modified()) {
                if !w.save_doc(d) { break; }
            }
        });
        act!("close-tab", |w| { if let Some(d) = w.current() { w.close_doc(&d); } });
        act!("quit", |w| { w.window.close(); });

        act!("undo", |w| { if let Some(d) = w.current() { if d.buffer.can_undo() { d.buffer.undo(); } } });
        act!("redo", |w| { if let Some(d) = w.current() { if d.buffer.can_redo() { d.buffer.redo(); } } });
        act!("cut", |w| {
            if let Some(e) = w.focused_editable() { e.cut_clipboard(); }
            else if let Some(d) = w.current() {
                d.buffer.cut_clipboard(&gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD), true);
            }
        });
        act!("copy", |w| {
            if let Some(e) = w.focused_editable() { e.copy_clipboard(); }
            else if let Some(d) = w.current() {
                d.buffer.copy_clipboard(&gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD));
            }
        });
        act!("paste", |w| {
            if let Some(e) = w.focused_editable() { e.paste_clipboard(); }
            else if let Some(d) = w.current() {
                d.buffer.paste_clipboard(&gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD), None, true);
            }
        });
        act!("select-all", |w| {
            if let Some(e) = w.focused_editable() { e.select_region(0, -1); }
            else if let Some(d) = w.current() {
                let (s, e) = d.buffer.bounds();
                d.buffer.select_range(&s, &e);
            }
        });
        act!("delete-line", |w| { if let Some(d) = w.current() { delete_line(&d); } });
        act!("duplicate-line", |w| { if let Some(d) = w.current() { duplicate_line(&d); } });
        act!("upper", |w| { if let Some(d) = w.current() { transform_selection(&d, |s| s.to_uppercase()); } });
        act!("lower", |w| { if let Some(d) = w.current() { transform_selection(&d, |s| s.to_lowercase()); } });
        act!("sort-lines", |w| {
            if let Some(d) = w.current() {
                transform_selection(&d, |s| {
                    let mut lines: Vec<&str> = s.split('\n').collect();
                    lines.sort();
                    lines.join("\n")
                });
            }
        });

        act!("find", |w| {
            let sel = w.current().and_then(|d| d.buffer.selection_bounds().and_then(|(s, e)| d.buffer.text(&s, &e, false)));
            w.findbar().show(false, sel.map(|s| s.to_string()));
        });
        act!("replace", |w| {
            let sel = w.current().and_then(|d| d.buffer.selection_bounds().and_then(|(s, e)| d.buffer.text(&s, &e, false)));
            w.findbar().show(true, sel.map(|s| s.to_string()));
        });
        act!("find-next", |w| { w.findbar().find_next(false); });
        act!("find-prev", |w| { w.findbar().find_prev(); });
        act!("goto-line", |w| { w.goto_line_dialog(); });

        act!("zoom-in", |w| { w.zoom(1); });
        act!("zoom-out", |w| { w.zoom(-1); });
        act!("zoom-reset", |w| { w.with_config(|c| c.font_size = 11); });
        act!("next-tab", |w| { w.notebook.next_page(); });
        act!("prev-tab", |w| { w.notebook.prev_page(); });
        act!("about", |w| { w.about_dialog(); });

        // Aksi toggle (centang di menu).
        macro_rules! toggle {
            ($name:expr, $init:expr, |$c:ident, $v:ident| $body:block) => {{
                let action = gio::SimpleAction::new_stateful($name, None, &$init.to_variant());
                let weak = Rc::downgrade(self);
                action.connect_activate(move |a, _| {
                    let cur = a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
                    let $v = !cur;
                    a.set_state(&$v.to_variant());
                    if let Some(w) = weak.upgrade() {
                        w.with_config(|$c| $body);
                    }
                });
                self.window.add_action(&action);
            }};
        }
        let cfg = self.config.borrow().clone();
        toggle!("toggle-statusbar", cfg.statusbar, |c, v| { c.statusbar = v; });
        toggle!("toggle-line-numbers", cfg.line_numbers, |c, v| { c.line_numbers = v; });
        toggle!("toggle-wrap", cfg.wrap, |c, v| { c.wrap = v; });
        toggle!("toggle-highlight-line", cfg.highlight_line, |c, v| { c.highlight_line = v; });
        toggle!("toggle-dark", cfg.dark, |c, v| { c.dark = v; c.scheme = "auto".to_string(); });
        toggle!("toggle-spaces", cfg.use_spaces, |c, v| { c.use_spaces = v; });

        // Lebar tab (radio).
        let action = gio::SimpleAction::new_stateful(
            "tab-width",
            Some(glib::VariantTy::STRING),
            &cfg.tab_width.to_string().to_variant(),
        );
        let weak = Rc::downgrade(self);
        action.connect_activate(move |a, p| {
            let Some(p) = p.and_then(|p| p.get::<String>()) else { return };
            a.set_state(&p.to_variant());
            if let (Some(w), Ok(n)) = (weak.upgrade(), p.parse::<u32>()) {
                w.with_config(|c| c.tab_width = n);
            }
        });
        self.window.add_action(&action);

        // ── aksi tambahan ──
        act!("reload", |w| { w.reload_action(None); });
        act!("move-up", |w| { if let Some(d) = w.current() { move_lines(&d, true); } });
        act!("move-down", |w| { if let Some(d) = w.current() { move_lines(&d, false); } });
        act!("toggle-comment", |w| { if let Some(d) = w.current() { toggle_comment(&d); } });
        act!("clear-recent", |_w| { session::clear_recent(); _w.rebuild_recent(); });

        macro_rules! act_s {
            ($name:expr, |$w:ident, $p:ident| $body:block) => {{
                let action = gio::SimpleAction::new($name, Some(glib::VariantTy::STRING));
                let weak = Rc::downgrade(self);
                action.connect_activate(move |_, param| {
                    let Some($p) = param.and_then(|p| p.get::<String>()) else { return };
                    if let Some($w) = weak.upgrade() {
                        $body
                    }
                });
                self.window.add_action(&action);
            }};
        }
        act_s!("open-recent", |w, p| { w.open_files(&[PathBuf::from(p)]); });
        act_s!("goto-tab", |w, p| { if let Ok(n) = p.parse::<u32>() { w.goto_tab(n); } });
        act_s!("enc-save", |w, p| { if let Some(e) = Encoding::from_id(&p) { w.set_save_encoding(e); } });
        act_s!("enc-reload", |w, p| { if let Some(e) = Encoding::from_id(&p) { w.reload_action(Some(e)); } });

        toggle!("toggle-trim", cfg.trim_trailing, |c, v| { c.trim_trailing = v; });
        toggle!("toggle-final-newline", cfg.final_newline, |c, v| { c.final_newline = v; });

        // Skema warna (radio).
        let action = gio::SimpleAction::new_stateful(
            "scheme",
            Some(glib::VariantTy::STRING),
            &cfg.scheme.to_variant(),
        );
        let weak = Rc::downgrade(self);
        action.connect_activate(move |a, p| {
            let Some(p) = p.and_then(|p| p.get::<String>()) else { return };
            a.set_state(&p.to_variant());
            if let Some(w) = weak.upgrade() {
                w.with_config(|c| c.scheme = p);
            }
        });
        self.window.add_action(&action);
    }
}

// ───────────────────────── operasi teks ─────────────────────────

/// Rentang baris yang dikenai aksi: seluruh seleksi (baris terakhir tidak ikut kalau
/// seleksi berakhir tepat di awal baris), atau baris kursor kalau tidak ada seleksi.
fn line_span(buf: &sourceview4::Buffer) -> (i32, i32) {
    match buf.selection_bounds() {
        Some((s, e)) => {
            let first = s.line();
            let mut last = e.line();
            if last > first && e.line_offset() == 0 {
                last -= 1;
            }
            (first, last)
        }
        None => {
            let l = buf.iter_at_mark(&crate::document::insert_mark(buf)).line();
            (l, l)
        }
    }
}

fn line_text(buf: &sourceview4::Buffer, line: i32) -> String {
    let s = buf.iter_at_line(line);
    let mut e = s;
    if !e.ends_line() {
        e.forward_to_line_end();
    }
    buf.text(&s, &e, false).map(|t| t.to_string()).unwrap_or_default()
}

fn delete_line(doc: &Document) {
    let buf = &doc.buffer;
    let (first, last) = line_span(buf);
    let mut start = buf.iter_at_line(first);
    let mut end = buf.iter_at_line(last);
    buf.begin_user_action();
    if !end.forward_line() {
        // Baris terakhir: ikut hapus newline sebelumnya.
        end = buf.end_iter();
        if first > 0 {
            start.backward_char();
        }
    }
    buf.delete(&mut start, &mut end);
    buf.end_user_action();
}

fn duplicate_line(doc: &Document) {
    let buf = &doc.buffer;
    let (first, last) = line_span(buf);
    let start = buf.iter_at_line(first);
    let mut end = buf.iter_at_line(last);
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    let text = buf.text(&start, &end, false).map(|t| t.to_string()).unwrap_or_default();
    buf.begin_user_action();
    buf.insert(&mut end, &format!("\n{text}"));
    buf.end_user_action();
}

/// Pindahkan baris kursor / seluruh baris terpilih satu baris ke atas atau bawah.
fn move_lines(doc: &Document, up: bool) {
    let buf = &doc.buffer;
    let (first, last) = line_span(buf);
    if (up && first == 0) || (!up && last + 1 >= buf.line_count()) {
        return;
    }
    let had_sel = buf.selection_bounds().is_some();
    let col = buf.iter_at_mark(&crate::document::insert_mark(buf)).line_offset();
    let n = last - first;
    let start = buf.iter_at_line(first);
    let mut end = buf.iter_at_line(last);
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    let block = buf.text(&start, &end, false).map(|t| t.to_string()).unwrap_or_default();
    buf.begin_user_action();
    let new_first;
    if up {
        // Buang "\n" + blok, lalu sisipkan "blok\n" di awal baris sebelumnya.
        let mut a = start;
        a.backward_char();
        let mut b = end;
        buf.delete(&mut a, &mut b);
        let mut ins = buf.iter_at_line(first - 1);
        buf.insert(&mut ins, &format!("{block}\n"));
        new_first = first - 1;
    } else {
        // Buang blok + newline-nya, lalu sisipkan "\nblok" di akhir baris berikutnya.
        let mut a = start;
        let mut b = buf.iter_at_line(last + 1);
        buf.delete(&mut a, &mut b);
        let mut ins = buf.iter_at_line(first);
        if !ins.ends_line() {
            ins.forward_to_line_end();
        }
        buf.insert(&mut ins, &format!("\n{block}"));
        new_first = first + 1;
    }
    buf.end_user_action();
    if had_sel {
        let a = buf.iter_at_line(new_first);
        let mut b = buf.iter_at_line(new_first + n);
        if !b.ends_line() {
            b.forward_to_line_end();
        }
        buf.select_range(&a, &b);
    } else {
        let it = buf.iter_at_line_offset(new_first, col);
        buf.place_cursor(&it);
    }
    let mark = crate::document::insert_mark(buf);
    doc.view.scroll_to_mark(&mark, 0.1, false, 0.5, 0.5);
}

/// Awalan komentar satu baris menurut bahasa aktif (None = bahasa tidak dikenal).
fn comment_prefix(doc: &Document) -> Option<&'static str> {
    let name = doc.buffer.language().and_then(|l| l.name()).map(|n| n.to_lowercase())?;
    Some(match name.as_str() {
        "python" | "python 3" | "sh" | "yaml" | "toml" | "ruby" | "perl" | "makefile" | "cmake"
        | "dockerfile" | "r" | "awk" | ".desktop" | "gdb" => "# ",
        "c" | "c++" | "c/objc header" | "objective-c" | "rust" | "javascript" | "typescript"
        | "java" | "go" | "c#" | "php" | "kotlin" | "swift" | "scala" | "dart" | "vala" => "// ",
        "sql" | "lua" | "haskell" => "-- ",
        "latex" | "matlab" => "% ",
        ".ini" => "; ",
        _ => return None,
    })
}

/// Komentari / batalkan komentar baris kursor atau seluruh baris terpilih.
fn toggle_comment(doc: &Document) {
    let Some(prefix) = comment_prefix(doc) else { return };
    let marker = prefix.trim_end();
    let buf = &doc.buffer;
    let (first, last) = line_span(buf);
    let (mut any, mut all) = (false, true);
    for l in first..=last {
        let t = line_text(buf, l);
        if t.trim().is_empty() {
            continue;
        }
        any = true;
        if !t.trim_start().starts_with(marker) {
            all = false;
            break;
        }
    }
    if !any {
        return;
    }
    buf.begin_user_action();
    for l in first..=last {
        let t = line_text(buf, l);
        if t.trim().is_empty() {
            continue;
        }
        let trimmed = t.trim_start();
        let indent = (t.chars().count() - trimmed.chars().count()) as i32;
        if all {
            let mut n = marker.chars().count() as i32;
            if trimmed[marker.len()..].starts_with(' ') {
                n += 1;
            }
            let mut a = buf.iter_at_line_offset(l, indent);
            let mut b = buf.iter_at_line_offset(l, indent + n);
            buf.delete(&mut a, &mut b);
        } else {
            let mut it = buf.iter_at_line_offset(l, indent);
            buf.insert(&mut it, prefix);
        }
    }
    buf.end_user_action();
}

fn transform_selection(doc: &Document, f: impl Fn(&str) -> String) {
    let buf = &doc.buffer;
    let Some((mut s, mut e)) = buf.selection_bounds() else { return };
    let Some(text) = buf.text(&s, &e, false) else { return };
    let new = f(text.as_str());
    let off = s.offset();
    buf.begin_user_action();
    buf.delete(&mut s, &mut e);
    buf.insert(&mut s, &new);
    buf.end_user_action();
    let a = buf.iter_at_offset(off);
    let b = buf.iter_at_offset(off + new.chars().count() as i32);
    buf.select_range(&a, &b);
}

// ───────────────────────── menu ─────────────────────────

fn section(items: &[(&str, &str)]) -> gio::Menu {
    let m = gio::Menu::new();
    for (label, action) in items {
        m.append(Some(label), Some(action));
    }
    m
}

fn build_menu(recent: &gio::Menu) -> gio::Menu {
    let root = gio::Menu::new();

    let file = gio::Menu::new();
    file.append_section(None, &section(&[("_Baru", "win.new"), ("_Buka…", "win.open")]));
    let recent_sec = gio::Menu::new();
    recent_sec.append_submenu(Some("Berkas _Terbaru"), recent);
    file.append_section(None, &recent_sec);
    file.append_section(
        None,
        &section(&[("_Simpan", "win.save"), ("Simpan _Sebagai…", "win.save-as"), ("Simpan Semu_a", "win.save-all")]),
    );
    let reload_sec = gio::Menu::new();
    reload_sec.append(Some("Muat _Ulang"), Some("win.reload"));
    let enc_reload = gio::Menu::new();
    for e in Encoding::ALL {
        enc_reload.append(Some(e.label()), Some(&format!("win.enc-reload::{}", e.id())));
    }
    reload_sec.append_submenu(Some("Buka Ulang dengan _Encoding"), &enc_reload);
    file.append_section(None, &reload_sec);
    file.append_section(None, &section(&[("_Tutup Tab", "win.close-tab"), ("_Keluar", "win.quit")]));
    root.append_submenu(Some("_Berkas"), &file);

    let edit = gio::Menu::new();
    edit.append_section(None, &section(&[("_Urungkan", "win.undo"), ("_Ulangi", "win.redo")]));
    edit.append_section(
        None,
        &section(&[("_Potong", "win.cut"), ("S_alin", "win.copy"), ("_Tempel", "win.paste"), ("Pilih _Semua", "win.select-all")]),
    );
    edit.append_section(
        None,
        &section(&[
            ("_Hapus Baris", "win.delete-line"),
            ("_Duplikat Baris", "win.duplicate-line"),
            ("Huruf _Besar", "win.upper"),
            ("Huruf _Kecil", "win.lower"),
            ("Urutkan Baris", "win.sort-lines"),
            ("Pindah Baris ke _Atas", "win.move-up"),
            ("Pindah Baris ke Ba_wah", "win.move-down"),
            ("_Komentari / Batalkan Komentar", "win.toggle-comment"),
        ]),
    );
    root.append_submenu(Some("_Edit"), &edit);

    let view = gio::Menu::new();
    view.append_section(
        None,
        &section(&[
            ("Bilah _Status", "win.toggle-statusbar"),
            ("_Nomor Baris", "win.toggle-line-numbers"),
            ("_Bungkus Baris", "win.toggle-wrap"),
            ("_Sorot Baris Aktif", "win.toggle-highlight-line"),
            ("Tema _Gelap", "win.toggle-dark"),
        ]),
    );
    view.append_section(
        None,
        &section(&[("Perbesar", "win.zoom-in"), ("Perkecil", "win.zoom-out"), ("Ukuran Normal", "win.zoom-reset")]),
    );
    let schemes = gio::Menu::new();
    for (label, id) in [
        ("Otomatis (ikut tema)", "auto"),
        ("Oblivion", "oblivion"),
        ("Cobalt", "cobalt"),
        ("Kate", "kate"),
        ("Solarized Gelap", "solarized-dark"),
        ("Solarized Terang", "solarized-light"),
        ("Tango", "tango"),
        ("Classic", "classic"),
    ] {
        schemes.append(Some(label), Some(&format!("win.scheme::{id}")));
    }
    let scheme_sec = gio::Menu::new();
    scheme_sec.append_submenu(Some("_Skema Warna"), &schemes);
    view.append_section(None, &scheme_sec);
    root.append_submenu(Some("_Tampilan"), &view);

    let search = gio::Menu::new();
    search.append_section(None, &section(&[("_Cari…", "win.find"), ("_Ganti…", "win.replace")]));
    search.append_section(
        None,
        &section(&[("Cari _Berikutnya", "win.find-next"), ("Cari _Sebelumnya", "win.find-prev"), ("Ke _Baris…", "win.goto-line")]),
    );
    root.append_submenu(Some("_Cari"), &search);

    let tools = gio::Menu::new();
    tools.append_section(
        None,
        &section(&[
            ("Pakai _Spasi (bukan Tab)", "win.toggle-spaces"),
            ("Hapus Spasi di Ujung Baris saat Simpan", "win.toggle-trim"),
            ("Akhiri File dengan Baris Baru saat Simpan", "win.toggle-final-newline"),
        ]),
    );
    let enc_save = gio::Menu::new();
    for e in Encoding::ALL {
        enc_save.append(Some(e.label()), Some(&format!("win.enc-save::{}", e.id())));
    }
    let enc_sec = gio::Menu::new();
    enc_sec.append_submenu(Some("Encoding _Simpan"), &enc_save);
    tools.append_section(None, &enc_sec);
    tools.append_section(
        None,
        &section(&[("Lebar Tab: 2", "win.tab-width::2"), ("Lebar Tab: 4", "win.tab-width::4"), ("Lebar Tab: 8", "win.tab-width::8")]),
    );
    tools.append_section(None, &section(&[("Tab Berikutnya", "win.next-tab"), ("Tab Sebelumnya", "win.prev-tab")]));
    root.append_submenu(Some("_Alat"), &tools);

    let help = gio::Menu::new();
    help.append(Some("_Tentang VallenText"), Some("win.about"));
    root.append_submenu(Some("Ban_tuan"), &help);

    root
}
