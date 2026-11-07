//! Mode ukur: jalankan dengan  VALLENTEXT_TRACE=1 vallentext  untuk melihat
//! waktu tiap tahap startup/pembukaan file di terminal. Mati = nol biaya.

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static ON: OnceLock<bool> = OnceLock::new();

pub fn init() {
    let _ = START.set(Instant::now());
    let _ = ON.set(std::env::var_os("VALLENTEXT_TRACE").is_some());
}

pub fn enabled() -> bool {
    *ON.get().unwrap_or(&false)
}

pub fn mark(label: &str) {
    if !enabled() {
        return;
    }
    if let Some(t) = START.get() {
        eprintln!("[trace] {:>7.1} ms  {label}", t.elapsed().as_secs_f64() * 1000.0);
    }
}
