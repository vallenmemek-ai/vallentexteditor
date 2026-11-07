//! VallenText — editor teks ringan ala Pluma (Rust + GTK3 + GtkSourceView).

mod app;
mod config;
mod document;
mod file_io;
mod findbar;
mod session;
mod trace;
mod window;

fn main() {
    trace::init();
    // Lewati koneksi ke bus aksesibilitas (AT-SPI): memangkas waktu start dan RAM.
    // Pengguna screen reader: jalankan dengan VALLENTEXT_A11Y=1.
    if std::env::var_os("VALLENTEXT_A11Y").is_none() && std::env::var_os("NO_AT_BRIDGE").is_none() {
        std::env::set_var("NO_AT_BRIDGE", "1");
    }
    trace::mark("main() mulai");
    std::process::exit(app::run());
}
