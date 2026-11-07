# VallenText

Editor teks ringan ala Pluma — Rust + GTK3 + GtkSourceView 4. Binary release ±600 KB.

## Build

```bash
./build.sh            # build release → dist/vallentext
./build.sh install    # pasang ke ~/.local (bin + ikon + menu aplikasi)
./build.sh default    # jadikan app bawaan untuk .txt .py .js .md .rs dst
./build.sh run        # jalankan versi debug
./build.sh uninstall
```

Dependency (Debian/Ubuntu/Mint):
`sudo apt install build-essential pkg-config libgtk-3-dev libgtksourceview-4-dev`

## Icon

Ikon ada di `assets/icons/`: `vallentext-16 … 512.png` (sudut transparan). Master-nya `assets/icons/source/vallentext-source.png` — kalau ganti ikon, buat ulang semua ukuran dari file itu (contoh dengan ImageMagick):

```bash
for s in 16 24 32 48 64 128 256 512; do
  convert assets/icons/source/vallentext-source.png -resize ${s}x${s} -strip assets/icons/vallentext-$s.png
done
```

Aturan penting: **nama file `vallentext-<ukuran>.png` dan ukuran pikselnya harus standar** (16, 22, 24, 32, 48, 64, 96, 128, 256, 512). `build.sh install` memasang tiap PNG ke `hicolor/<ukuran>x<ukuran>/apps/`; ukuran non-standar (misal 1254×1254) dilewati karena tema ikon mengabaikannya.
Ikon 128 px (`vallentext-128.png`) juga **ditanam di binary** (`include_bytes!`), jadi ikon jendela, taskbar, dan dialog Tentang selalu yang terbaru walau belum `./build.sh install`. Entri menu aplikasi tetap memakai ikon terpasang, jadi setelah ganti ikon jalankan `./build.sh install` lagi.

Ganti nama app: cari `vallentext` di `Cargo.toml`, `build.sh`, `data/vallentext.desktop`, dan `APP_ID`/`ICON_NAME` di `src/app.rs`.

## Performa

- Pilihan bahasa dibangun malas (saat pertama diklik), bukan saat start — sebelumnya ratusan definisi bahasa di-parse tiap app dibuka.
- Jendela tampil dulu, baru file dibaca (idle); update status bar digabung per siklus idle, bukan per ketikan.
- File >3 MB: syntax highlighting otomatis dimatikan.
- Koneksi AT-SPI dilewati (`NO_AT_BRIDGE=1`); screen reader: jalankan dengan `VALLENTEXT_A11Y=1`.
- Release: `opt-level=3`, LTO, `codegen-units=1`, `panic=abort`, strip.
- Bar cari & ganti dibangun malas (saat pertama Ctrl+F). Baca/simpan file tanpa salinan memori berlebih (BOM & CRLF dibuang di tempat; simpan UTF-8 langsung dari buffer).
- Mode ukur: `VALLENTEXT_TRACE=1 vallentext file.txt` mencetak waktu tiap tahap (GTK siap, frame pertama, file dimuat).
- Single-instance: membuka file kedua langsung muncul di jendela yang sudah ada.

## Struktur

```
vallentext/
├─ build.sh            skrip build/install
├─ Cargo.toml / .lock  (Cargo.lock di-commit, jangan dihapus)
├─ assets/icons/       icon app
├─ data/vallentext.desktop   entri menu aplikasi
└─ src/
   ├─ main.rs          entry point
   ├─ app.rs           Application, shortcut, single-instance, ikon tertanam
   ├─ window.rs        jendela, menu, tab, status bar, semua aksi
   ├─ document.rs      satu tab = buffer + view + info file
   ├─ findbar.rs       cari & ganti
   ├─ file_io.rs       baca/tulis file (encoding, CRLF, simpan atomik)
   ├─ session.rs       sesi terakhir, berkas terbaru, autosave pemulihan
   └─ config.rs        pengaturan ~/.config/vallentext/config.ini
```

Nambah fitur: aksi baru → `act!(...)` di `setup_actions()` + 1 baris di `build_menu()` (window.rs) + shortcut di `app.rs`.
Data sesi/pemulihan disimpan di `~/.local/share/vallentext/`.

## Fitur

Tab banyak file · buka/simpan/simpan semua (simpan atomik, symlink aman) · syntax highlighting + pilih bahasa manual · cari & ganti (regex, case) · ke baris · urungkan/ulangi · hapus/duplikat/pindah baris (mendukung banyak baris terpilih) · komentari baris · huruf besar/kecil · urutkan baris · bungkus baris, nomor baris, sorot baris aktif · tema gelap/terang + pilihan skema warna · zoom (Ctrl +/−/0, Ctrl+scroll) · lebar tab & spasi/tab · deteksi encoding (UTF-8, UTF-16, Windows-1252, ISO-8859-1) + buka ulang/simpan dengan encoding pilihan · LF/CRLF (penanda `*` kalau file campuran) · seret-lepas file · satu instance · ingat ukuran jendela & pengaturan · berkas terbaru · pulihkan tab sesi terakhir · autosave pemulihan dokumen yang belum disimpan (tiap 30 detik) · deteksi file berubah di disk · opsi hapus spasi ujung baris & akhiri file dengan baris baru saat simpan · status bar: jumlah baris dan karakter terpilih.

## Shortcut

| Aksi | Shortcut |
|---|---|
| Baru / Buka / Simpan / Simpan Sebagai | Ctrl+N / O / S / Shift+S |
| Simpan Semua | Ctrl+Alt+S |
| Muat Ulang dari disk | F5 |
| Tab berikutnya / sebelumnya | Ctrl+Tab, Ctrl+PgDn / Ctrl+Shift+Tab, Ctrl+PgUp |
| Pindah ke tab 1–9 | Alt+1 … Alt+9 |
| Hapus / duplikat baris | Ctrl+D / Ctrl+Shift+D |
| Pindah baris ke atas / bawah | Alt+↑ / Alt+↓ |
| Komentari / batalkan komentar | Ctrl+/ |
| Huruf besar / kecil | Ctrl+Alt+U / Ctrl+Alt+L |
| Urutkan baris | F9 |
| Cari / Ganti / Berikutnya / Sebelumnya / Ke baris | Ctrl+F / H / G / Shift+G / L |
