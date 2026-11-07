#!/usr/bin/env bash
# VallenText — skrip build.
#
#   ./build.sh            build release → dist/vallentext
#   ./build.sh run        build debug lalu jalankan
#   ./build.sh install    build + pasang ke ~/.local (bin, ikon, menu aplikasi)
#   ./build.sh default    jadikan VallenText aplikasi bawaan untuk .txt .py .js .md .rs dst
#   ./build.sh uninstall  hapus yang dipasang oleh install
#   ./build.sh deps       cek dependency sistem
#   ./build.sh clean      hapus target/ dan dist/
#
# Variabel opsional:  PREFIX=/usr/local sudo ./build.sh install

set -euo pipefail
cd "$(dirname "$0")"

APP=vallentext
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
APPS_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor"

c_ok()   { printf '\033[32m✔\033[0m %s\n' "$*"; }
c_err()  { printf '\033[31m✘\033[0m %s\n' "$*" >&2; }
c_info() { printf '\033[36m➜\033[0m %s\n' "$*"; }

check_deps() {
  local missing=0
  if ! command -v cargo >/dev/null 2>&1; then
    c_err "cargo (Rust) belum terpasang. Pasang lewat https://rustup.rs atau paket distro (apt install cargo)."
    missing=1
  fi
  if ! command -v pkg-config >/dev/null 2>&1; then
    c_err "pkg-config belum terpasang."
    missing=1
  else
    for lib in gtk+-3.0 gtksourceview-4; do
      if ! pkg-config --exists "$lib"; then
        c_err "library '$lib' tidak ditemukan."
        missing=1
      fi
    done
  fi
  if [ "$missing" -ne 0 ]; then
    cat >&2 <<'MSG'

Pasang dependency dulu:
  Debian/Ubuntu/Mint : sudo apt install build-essential pkg-config libgtk-3-dev libgtksourceview-4-dev
  Fedora             : sudo dnf install gcc pkgconf-pkg-config gtk3-devel gtksourceview4-devel
  Arch               : sudo pacman -S base-devel gtk3 gtksourceview4
MSG
    exit 1
  fi
  c_ok "dependency lengkap ($(cargo --version | cut -d' ' -f1-2))"
}

build_release() {
  check_deps
  c_info "build release…"
  cargo build --release
  mkdir -p dist
  cp "target/release/$APP" "dist/$APP"
  c_ok "selesai → dist/$APP ($(du -h "dist/$APP" | cut -f1))"
}

png_width() {
  # Baca lebar PNG dari header (byte 16-19).
  od -An -tu1 -j16 -N4 "$1" | awk '{print $1*16777216 + $2*65536 + $3*256 + $4}'
}

ICON_SIZES="16 22 24 32 48 64 96 128 256 512"

install_icons() {
  local found=0
  # Bersihkan sisa instalasi lama yang ukurannya tidak standar (mis. 1254x1254):
  # tema ikon mengabaikan folder seperti itu, makanya ikon baru tidak muncul.
  local old d w
  for old in "$ICON_DIR"/*x*/apps/$APP.png; do
    [ -f "$old" ] || continue
    d="$(basename "$(dirname "$(dirname "$old")")")"; w="${d%%x*}"
    case " $ICON_SIZES " in *" $w "*) ;; *) rm -f "$old";; esac
  done
  if [ -f assets/icons/$APP.svg ]; then
    install -Dm644 assets/icons/$APP.svg "$ICON_DIR/scalable/apps/$APP.svg"
    found=1
  fi
  for png in assets/icons/*.png; do
    [ -f "$png" ] || continue
    w="$(png_width "$png")"
    case " $ICON_SIZES " in
      *" $w "*) install -Dm644 "$png" "$ICON_DIR/${w}x${w}/apps/$APP.png"; found=1 ;;
      *) c_err "dilewati $png: ukuran ${w}px bukan ukuran ikon standar ($ICON_SIZES)" ;;
    esac
  done
  if [ "$found" -eq 0 ]; then
    c_err "icon belum ada di assets/icons/ (taruh vallentext-<ukuran>.png). Lanjut tanpa icon."
  else
    c_ok "icon terpasang"
  fi
}

refresh_caches() {
  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t -f "$ICON_DIR" 2>/dev/null || true
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$APPS_DIR" 2>/dev/null || true
}

do_install() {
  build_release
  install -Dm755 "dist/$APP" "$BIN_DIR/$APP"
  # Exec pakai path absolut: ~/.local/bin sering belum ada di PATH sesi desktop.
  mkdir -p "$APPS_DIR"
  sed "s|^Exec=.*|Exec=$BIN_DIR/$APP %F|" data/$APP.desktop > "$APPS_DIR/$APP.desktop"
  chmod 644 "$APPS_DIR/$APP.desktop"
  install_icons
  refresh_caches
  c_ok "terpasang di $BIN_DIR/$APP — cari 'VallenText' di menu Aplikasi → Aksesori"
  c_info "kalau belum muncul di menu MATE: jalankan  mate-panel --replace &  (atau logout/login)"
  case ":$PATH:" in *":$BIN_DIR:"*) ;; *) c_info "catatan: $BIN_DIR belum ada di PATH";; esac
}

do_default() {
  command -v xdg-mime >/dev/null 2>&1 || { c_err "xdg-mime tidak ada (apt install xdg-utils)."; exit 1; }
  [ -f "$APPS_DIR/$APP.desktop" ] || { c_err "belum terpasang — jalankan ./build.sh install dulu."; exit 1; }
  local n=0 m
  for m in $(grep '^MimeType=' data/$APP.desktop | cut -d= -f2 | tr ';' ' '); do
    xdg-mime default "$APP.desktop" "$m" && n=$((n+1))
  done
  c_ok "VallenText jadi aplikasi bawaan untuk $n tipe file (buka file dobel-klik → VallenText)"
  c_info "balikin ke Pluma: xdg-mime default pluma.desktop text/plain  (atau klik kanan file → Properties → Open With)"
}

do_uninstall() {
  rm -f "$BIN_DIR/$APP" "$APPS_DIR/$APP.desktop"
  rm -f "$ICON_DIR"/*/apps/$APP.png "$ICON_DIR"/scalable/apps/$APP.svg
  refresh_caches
  c_ok "dihapus (pengaturan di ~/.config/vallentext dibiarkan)"
}

case "${1:-build}" in
  build)     build_release ;;
  run)       check_deps; shift || true; cargo run -- "$@" ;;
  install)   do_install ;;
  default)   do_default ;;
  uninstall) do_uninstall ;;
  deps)      check_deps ;;
  clean)     cargo clean; rm -rf dist; c_ok "bersih" ;;
  -h|--help|help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//' ;;
  *) c_err "perintah tidak dikenal: $1 (coba ./build.sh help)"; exit 1 ;;
esac
