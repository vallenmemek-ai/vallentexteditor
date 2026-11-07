//! Application: single-instance (buka file kedua → muncul di jendela yang sama).

use crate::window::VallenTextWindow;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub const APP_ID: &str = "dev.vallentext.Editor";
pub const ICON_NAME: &str = "vallentext";

/// Ikon 128 px ditanam di binary: jendela, taskbar, dan dialog Tentang selalu memakai
/// ikon terbaru walau belum `./build.sh install` atau cache ikon sistem masih lama.
static ICON_PNG: &[u8] = include_bytes!("../assets/icons/vallentext-128.png");

pub fn icon_pixbuf() -> Option<gtk::gdk_pixbuf::Pixbuf> {
    gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(ICON_PNG)).ok()
}

thread_local! {
    static WINDOW: RefCell<Option<Rc<VallenTextWindow>>> = RefCell::new(None);
}

fn window_for(app: &gtk::Application) -> Rc<VallenTextWindow> {
    WINDOW.with(|slot| {
        slot.borrow_mut().get_or_insert_with(|| VallenTextWindow::new(app)).clone()
    })
}

pub fn run() -> i32 {
    glib::set_application_name("VallenText");
    glib::set_prgname(Some("vallentext"));
    let app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::HANDLES_OPEN);

    app.connect_startup(|app| {
        crate::trace::mark("GTK siap (startup)");
        if let Some(pb) = icon_pixbuf() {
            gtk::Window::set_default_icon(&pb);
        } else {
            gtk::Window::set_default_icon_name(ICON_NAME);
        }
        let accels: &[(&str, &[&str])] = &[
            ("win.new", &["<Primary>n"]),
            ("win.open", &["<Primary>o"]),
            ("win.save", &["<Primary>s"]),
            ("win.save-as", &["<Primary><Shift>s"]),
            ("win.close-tab", &["<Primary>w"]),
            ("win.quit", &["<Primary>q"]),
            ("win.undo", &["<Primary>z"]),
            ("win.redo", &["<Primary><Shift>z", "<Primary>y"]),
            ("win.cut", &["<Primary>x"]),
            ("win.copy", &["<Primary>c"]),
            ("win.paste", &["<Primary>v"]),
            ("win.select-all", &["<Primary>a"]),
            ("win.delete-line", &["<Primary>d"]),
            ("win.duplicate-line", &["<Primary><Shift>d"]),
            ("win.find", &["<Primary>f"]),
            ("win.replace", &["<Primary>h"]),
            ("win.find-next", &["<Primary>g", "F3"]),
            ("win.find-prev", &["<Primary><Shift>g", "<Shift>F3"]),
            ("win.goto-line", &["<Primary>l"]),
            ("win.zoom-in", &["<Primary>plus", "<Primary>equal", "<Primary>KP_Add"]),
            ("win.zoom-out", &["<Primary>minus", "<Primary>KP_Subtract"]),
            ("win.zoom-reset", &["<Primary>0"]),
            ("win.save-all", &["<Primary><Alt>s"]),
            ("win.reload", &["F5"]),
            ("win.next-tab", &["<Primary>Tab", "<Primary>Page_Down"]),
            ("win.prev-tab", &["<Primary><Shift>Tab", "<Primary>Page_Up"]),
            ("win.move-up", &["<Alt>Up"]),
            ("win.move-down", &["<Alt>Down"]),
            ("win.toggle-comment", &["<Primary>slash"]),
            ("win.upper", &["<Primary><Alt>u"]),
            ("win.lower", &["<Primary><Alt>l"]),
            ("win.sort-lines", &["F9"]),
            ("win.goto-tab::1", &["<Alt>1"]),
            ("win.goto-tab::2", &["<Alt>2"]),
            ("win.goto-tab::3", &["<Alt>3"]),
            ("win.goto-tab::4", &["<Alt>4"]),
            ("win.goto-tab::5", &["<Alt>5"]),
            ("win.goto-tab::6", &["<Alt>6"]),
            ("win.goto-tab::7", &["<Alt>7"]),
            ("win.goto-tab::8", &["<Alt>8"]),
            ("win.goto-tab::9", &["<Alt>9"]),
        ];
        for (action, keys) in accels {
            app.set_accels_for_action(action, keys);
        }
    });

    app.connect_activate(|app| {
        let win = window_for(app);
        crate::trace::mark("jendela dibuat");
        // Jendela + satu tab kosong tampil dulu; pemulihan sesi menyusul setelah frame
        // pertama tergambar (idle), jadi tidak menunda waktu muncul.
        let first = win.doc_count() == 0;
        if first {
            win.new_doc();
        }
        win.window.present();
        if first {
            let w = win.clone();
            glib::idle_add_local_once(move || w.restore_session());
        }
    });

    app.connect_open(|app, files, _| {
        let win = window_for(app);
        // Tampilkan jendela dulu, baru baca file → terasa langsung muncul.
        win.window.present();
        let paths: Vec<PathBuf> = files.iter().filter_map(|f| f.path()).collect();
        glib::idle_add_local_once(move || {
            win.open_files(&paths);
            if win.doc_count() == 0 {
                win.new_doc();
            }
        });
    });

    app.run()
}
