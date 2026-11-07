//! Data kecil di ~/.local/share/vallentext: sesi terakhir, daftar berkas terbaru,
//! dan salinan pemulihan (autosave) untuk dokumen yang belum disimpan.
//! Semua format teks biasa supaya tidak perlu crate tambahan.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 10;

fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
    Some(base.join("vallentext"))
}

fn ensure(dir: &Path) {
    let _ = fs::create_dir_all(dir);
}

// ───────────────────────── sesi ─────────────────────────

pub fn save_session(paths: &[PathBuf], current: usize) {
    let Some(d) = data_dir() else { return };
    ensure(&d);
    let mut s = format!("current={current}\n");
    for p in paths {
        let t = p.to_string_lossy();
        if !t.contains('\n') {
            s.push_str(&format!("file={t}\n"));
        }
    }
    let _ = fs::write(d.join("session.ini"), s);
}

pub fn load_session() -> (Vec<PathBuf>, usize) {
    let mut files = Vec::new();
    let mut current = 0usize;
    let Some(d) = data_dir() else { return (files, current) };
    let Ok(text) = fs::read_to_string(d.join("session.ini")) else { return (files, current) };
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("current=") {
            current = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("file=") {
            files.push(PathBuf::from(v));
        }
    }
    (files, current)
}

// ───────────────────────── berkas terbaru ─────────────────────────

pub fn load_recent() -> Vec<PathBuf> {
    let Some(d) = data_dir() else { return Vec::new() };
    let Ok(text) = fs::read_to_string(d.join("recent.txt")) else { return Vec::new() };
    text.lines().filter(|l| !l.is_empty()).map(PathBuf::from).filter(|p| p.exists()).take(MAX_RECENT).collect()
}

fn write_recent(list: &[PathBuf]) {
    let Some(d) = data_dir() else { return };
    ensure(&d);
    let s: String = list
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|t| !t.contains('\n'))
        .map(|t| t + "\n")
        .collect();
    let _ = fs::write(d.join("recent.txt"), s);
}

pub fn push_recent(p: &Path) {
    let mut v = load_recent();
    v.retain(|x| x != p);
    v.insert(0, p.to_path_buf());
    v.truncate(MAX_RECENT);
    write_recent(&v);
}

pub fn clear_recent() {
    write_recent(&[]);
}

// ───────────────────────── pemulihan (autosave) ─────────────────────────

fn rec_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("recovery"))
}

pub fn write_recovery(id: u32, orig: Option<&Path>, text: &str) {
    let Some(d) = rec_dir() else { return };
    ensure(&d);
    let meta = orig.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let _ = fs::write(d.join(format!("{id}.meta")), meta);
    let tmp = d.join(format!("{id}.txt.tmp"));
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(&tmp, d.join(format!("{id}.txt")));
    }
}

pub fn remove_recovery(id: u32) {
    let Some(d) = rec_dir() else { return };
    let _ = fs::remove_file(d.join(format!("{id}.txt")));
    let _ = fs::remove_file(d.join(format!("{id}.meta")));
}

pub struct Recovered {
    pub id: u32,
    pub path: Option<PathBuf>,
    pub text: String,
}

pub fn load_recoveries() -> Vec<Recovered> {
    let Some(d) = rec_dir() else { return Vec::new() };
    let Ok(rd) = fs::read_dir(&d) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("txt") {
            continue;
        }
        let Some(id) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(text) = fs::read_to_string(&p) else { continue };
        let meta = fs::read_to_string(d.join(format!("{id}.meta"))).unwrap_or_default();
        let path = if meta.trim().is_empty() { None } else { Some(PathBuf::from(meta.trim())) };
        out.push(Recovered { id, path, text });
    }
    out.sort_by_key(|r| r.id);
    out
}
