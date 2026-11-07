//! Baca/tulis file: deteksi encoding (UTF-8 / UTF-16 / Windows-1252 / ISO-8859-1) dan
//! akhir baris (LF / CRLF). Penyimpanan atomik (tulis ke file sementara lalu rename)
//! supaya file tidak rusak kalau listrik mati saat menyimpan.

use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    Utf8,
    Latin1,
    Windows1252,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    pub const ALL: [Encoding; 5] = [
        Encoding::Utf8,
        Encoding::Latin1,
        Encoding::Windows1252,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Latin1 => "ISO-8859-1",
            Encoding::Windows1252 => "Windows-1252",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
        }
    }

    /// ID pendek untuk nama aksi menu.
    pub fn id(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf8",
            Encoding::Latin1 => "latin1",
            Encoding::Windows1252 => "cp1252",
            Encoding::Utf16Le => "utf16le",
            Encoding::Utf16Be => "utf16be",
        }
    }

    pub fn from_id(s: &str) -> Option<Encoding> {
        Encoding::ALL.into_iter().find(|e| e.id() == s)
    }
}

pub struct Loaded {
    pub text: String,
    pub encoding: Encoding,
    pub crlf: bool,
    /// true kalau file campuran CRLF dan LF (akan diseragamkan saat disimpan).
    pub mixed_eol: bool,
}

/// Batas aman supaya tidak membuka file raksasa/biner tanpa sengaja.
/// (Isi file disalin ke buffer GTK, jadi pemakaian RAM ±2–3× ukuran file.)
const MAX_SIZE: u64 = 50 * 1024 * 1024;

/// Kode Unicode untuk byte 0x80–0x9F di Windows-1252.
const CP1252: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];

fn decode_byte(enc: Encoding, b: u8) -> char {
    if enc == Encoding::Windows1252 && (0x80..=0x9F).contains(&b) {
        char::from_u32(CP1252[(b - 0x80) as usize] as u32).unwrap_or('\u{FFFD}')
    } else {
        b as char
    }
}

fn encode_char(enc: Encoding, c: char) -> Option<u8> {
    let u = c as u32;
    match enc {
        Encoding::Windows1252 => {
            if u < 0x80 || (0xA0..=0xFF).contains(&u) {
                Some(u as u8)
            } else {
                CP1252.iter().position(|&x| x as u32 == u).map(|i| 0x80 + i as u8)
            }
        }
        _ => (u <= 0xFF).then_some(u as u8),
    }
}

fn decode_single(bytes: &[u8], enc: Encoding) -> String {
    bytes.iter().map(|&b| decode_byte(enc, b)).collect()
}

pub fn read_text(path: &Path) -> Result<Loaded, String> {
    read_text_as(path, None)
}

/// `forced`: paksa pakai encoding tertentu (menu "Buka Ulang dengan Encoding").
pub fn read_text_as(path: &Path, forced: Option<Encoding>) -> Result<Loaded, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_SIZE {
        return Err("File terlalu besar untuk dibuka (maks 50 MB).".into());
    }
    let mut bytes = fs::read(path).map_err(|e| e.to_string())?;

    // UTF-16 (BOM atau dipaksa): jangan dianggap biner walau banyak byte 0.
    let bom16 = if bytes.starts_with(&[0xFF, 0xFE]) {
        Some(Encoding::Utf16Le)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some(Encoding::Utf16Be)
    } else {
        None
    };
    if let Some(enc) = forced.or(bom16) {
        if matches!(enc, Encoding::Utf16Le | Encoding::Utf16Be) {
            if bom16 == Some(enc) {
                bytes.drain(..2);
            }
            if bytes.len() % 2 == 1 {
                bytes.pop();
            }
            let le = enc == Encoding::Utf16Le;
            let units = bytes.chunks_exact(2).map(|c| {
                if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }
            });
            let text: String = char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect();
            let crlf = text.contains("\r\n");
            let mixed_eol = crlf && text.replace("\r\n", "").contains('\n');
            let text = if crlf { text.replace("\r\n", "\n") } else { text };
            return Ok(Loaded { text, encoding: enc, crlf, mixed_eol });
        }
    }

    if bytes.iter().take(8192).any(|&b| b == 0) {
        return Err("File ini tampaknya biner, bukan teks.".into());
    }
    // BOM UTF-8: buang di tempat (tanpa salinan baru).
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }

    // CRLF → LF di tempat (satu lintasan, tanpa alokasi). Aman untuk UTF-8 maupun
    // encoding 1-byte karena \r dan \n adalah byte ASCII. Sekalian hitung LF "telanjang"
    // (tanpa CR) untuk mendeteksi file campuran.
    let crlf = bytes.windows(2).any(|w| w == b"\r\n");
    let mut bare_lf = 0usize;
    if crlf {
        let n = bytes.len();
        let (mut r, mut w) = (0, 0);
        let mut prev = 0u8;
        while r < n {
            let b = bytes[r];
            if b == b'\r' && r + 1 < n && bytes[r + 1] == b'\n' {
                prev = b;
                r += 1;
                continue;
            }
            if b == b'\n' && prev != b'\r' {
                bare_lf += 1;
            }
            bytes[w] = b;
            w += 1;
            r += 1;
            prev = b;
        }
        bytes.truncate(w);
    }
    let mixed_eol = crlf && bare_lf > 0;

    let (text, encoding) = match forced {
        None | Some(Encoding::Utf8) => match String::from_utf8(bytes) {
            Ok(s) => (s, Encoding::Utf8),
            Err(e) if forced == Some(Encoding::Utf8) => {
                (String::from_utf8_lossy(e.as_bytes()).into_owned(), Encoding::Utf8)
            }
            Err(e) => {
                let b = e.into_bytes();
                // Byte 0x80–0x9F hampir pasti Windows-1252 (tanda kutip melengkung dst).
                let enc = if b.iter().any(|&x| (0x80..=0x9F).contains(&x)) {
                    Encoding::Windows1252
                } else {
                    Encoding::Latin1
                };
                (decode_single(&b, enc), enc)
            }
        },
        Some(enc) => (decode_single(&bytes, enc), enc),
    };
    Ok(Loaded { text, encoding, crlf, mixed_eol })
}

pub fn write_text(path: &Path, text: &str, encoding: Encoding, crlf: bool) -> Result<(), String> {
    let body: Cow<str> = if crlf { Cow::Owned(text.replace('\n', "\r\n")) } else { Cow::Borrowed(text) };
    let bytes: Cow<[u8]> = match encoding {
        Encoding::Utf8 => match body {
            Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
            Cow::Owned(s) => Cow::Owned(s.into_bytes()),
        },
        Encoding::Latin1 | Encoding::Windows1252 => {
            let mut out = Vec::with_capacity(body.len());
            for c in body.chars() {
                match encode_char(encoding, c) {
                    Some(b) => out.push(b),
                    None => {
                        return Err(format!(
                            "Karakter “{c}” tidak bisa disimpan dalam {}. Ganti encoding lewat menu Alat → Encoding Simpan.",
                            encoding.label()
                        ))
                    }
                }
            }
            Cow::Owned(out)
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let le = encoding == Encoding::Utf16Le;
            let mut out = Vec::with_capacity(body.len() * 2 + 2);
            out.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
            for u in body.encode_utf16() {
                out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            Cow::Owned(out)
        }
    };

    // Kalau `path` symlink, tulis ke file tujuannya — jangan mengganti symlink-nya.
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = real.as_path();

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.vallentext-tmp"));

    let atomic = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            let _ = fs::set_permissions(&tmp, meta.permissions());
        }
        fs::rename(&tmp, path)
    })();

    if atomic.is_err() {
        let _ = fs::remove_file(&tmp);
        // Fallback: tulis langsung (misal folder tidak bisa dibuat file baru).
        fs::write(path, &*bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("vallentext-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn baca_bom_dan_crlf() {
        let p = tmp("crlf");
        fs::write(&p, b"\xef\xbb\xbfhalo\r\ndunia\r\nakhir").unwrap();
        let l = read_text(&p).unwrap();
        assert_eq!(l.text, "halo\ndunia\nakhir");
        assert!(l.crlf);
        assert!(!l.mixed_eol);
        assert_eq!(l.encoding, Encoding::Utf8);
        write_text(&p, &l.text, l.encoding, l.crlf).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"halo\r\ndunia\r\nakhir");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn akhir_baris_campuran_terdeteksi() {
        let p = tmp("mixed");
        fs::write(&p, b"a\r\nb\nc\r\n").unwrap();
        let l = read_text(&p).unwrap();
        assert_eq!(l.text, "a\nb\nc\n");
        assert!(l.crlf && l.mixed_eol);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn latin1_bolak_balik() {
        let p = tmp("lat");
        fs::write(&p, b"caf\xe9\nok").unwrap();
        let l = read_text(&p).unwrap();
        assert_eq!(l.encoding, Encoding::Latin1);
        assert_eq!(l.text, "caf\u{e9}\nok");
        write_text(&p, &l.text, l.encoding, l.crlf).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"caf\xe9\nok");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn windows1252_kutip_melengkung() {
        let p = tmp("cp");
        fs::write(&p, b"\x93halo\x94 \x80").unwrap();
        let l = read_text(&p).unwrap();
        assert_eq!(l.encoding, Encoding::Windows1252);
        assert_eq!(l.text, "\u{201C}halo\u{201D} \u{20AC}");
        write_text(&p, &l.text, l.encoding, l.crlf).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"\x93halo\x94 \x80");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn utf16_bom_bolak_balik() {
        let p = tmp("u16");
        let mut raw = vec![0xFF, 0xFE];
        for u in "hai\r\n😀".encode_utf16() {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        fs::write(&p, &raw).unwrap();
        let l = read_text(&p).unwrap();
        assert_eq!(l.encoding, Encoding::Utf16Le);
        assert_eq!(l.text, "hai\n😀");
        assert!(l.crlf);
        write_text(&p, &l.text, l.encoding, l.crlf).unwrap();
        assert_eq!(fs::read(&p).unwrap(), raw);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn lf_utf8_tidak_berubah_dan_biner_ditolak() {
        let p = tmp("lf");
        write_text(&p, "satu\ndua\u{1F600}\n", Encoding::Utf8, false).unwrap();
        let l = read_text(&p).unwrap();
        assert_eq!(l.text, "satu\ndua\u{1F600}\n");
        assert!(!l.crlf);
        fs::write(&p, b"abc\0def").unwrap();
        assert!(read_text(&p).is_err());
        let _ = fs::remove_file(p);
    }

    #[cfg(unix)]
    #[test]
    fn simpan_lewat_symlink_menjaga_symlink() {
        let real = tmp("real");
        let link = tmp("link");
        fs::write(&real, b"lama").unwrap();
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_text(&link, "baru", Encoding::Utf8, false).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"baru");
        let _ = fs::remove_file(link);
        let _ = fs::remove_file(real);
    }
}
