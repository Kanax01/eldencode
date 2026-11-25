#!/usr/bin/env bash
# eldencode installer — builds the release binary, lays down a self-contained
# ~/.eldencode tree (binary, config, backups), adds a PATH block to shell rc
# files and links eldencode into ~/.local/bin when that is usable.
set -euo pipefail

APP="eldencode"
SELF_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
ELDENCODE_HOME="${ELDENCODE_HOME:-$HOME/.eldencode}"
LINK_DIR=""
DO_BUILD=1
UNINSTALL=0
PURGE=0

usage() {
  cat <<'EOF'
usage: ./install.sh [options]

Builds eldencode from source (a fresh release build of the eldencode crate each
time), then installs it into ~/.eldencode (override with --home or
$ELDENCODE_HOME):
  ~/.eldencode/bin/eldencode   the release binary
  ~/.eldencode/config.toml     editable defaults (theme, process, auto-attach)
  ~/.eldencode/backups/        save copies made with `b` in the menu

The item catalog (weapons, armour, talismans, goods, ashes, magic) and the
offsets are compiled into the binary; there are no data files to update.

It also refreshes the path block in your shell rc files and links eldencode into
~/.local/bin when that directory exists or is on PATH.

Re-running refreshes only the binary; an existing config, backups and
~/.local/bin/eldencode are left untouched.

options:
  --home DIR     install under DIR instead of ~/.eldencode
  --link DIR     also symlink DIR/eldencode -> <home>/bin/eldencode, if absent
  --no-build     skip the fresh build and reuse target/release/eldencode
  --uninstall    remove the PATH block and the installed binary
  --purge        with --uninstall, delete the whole eldencode home
  -h, --help     show this help
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --home)
      ELDENCODE_HOME="${2:?--home needs a directory}"
      shift 2
      ;;
    --home=*)
      ELDENCODE_HOME="${1#*=}"
      shift
      ;;
    --link)
      LINK_DIR="${2:?--link needs a directory}"
      shift 2
      ;;
    --link=*)
      LINK_DIR="${1#*=}"
      shift
      ;;
    --no-build)
      DO_BUILD=0
      shift
      ;;
    --uninstall)
      UNINSTALL=1
      shift
      ;;
    --purge)
      PURGE=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "${ELDENCODE_HOME:-}" ]; then
  echo "error: --home cannot be empty" >&2
  exit 2
fi

case "$ELDENCODE_HOME" in
  "~") ELDENCODE_HOME="$HOME" ;;
  "~/"*) ELDENCODE_HOME="$HOME/${ELDENCODE_HOME#\~/}" ;;
esac

if [ "$PURGE" = 1 ] && [ "$UNINSTALL" = 0 ]; then
  echo "error: --purge only applies together with --uninstall" >&2
  exit 2
fi

INSTALLED_BIN="$ELDENCODE_HOME/bin/eldencode"
LOCAL_BIN="$HOME/.local/bin"
LOCAL_LINK="$LOCAL_BIN/eldencode"

# rc files that exist, or ~/.bashrc if none do
rc_files() {
  local found=()
  local f
  for f in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
    if [ -f "$f" ]; then
      found+=("$f")
    fi
  done
  if [ ${#found[@]} -eq 0 ]; then
    touch "$HOME/.bashrc"
    found=("$HOME/.bashrc")
  fi
  printf '%s\n' "${found[@]}"
}

# Remove only the marked block. Unlike a global blank-line collapse, this
# leaves the rest of the user's rc file byte-for-byte intact.
remove_path_block() {
  local file="$1"
  local quiet="${2:-}"
  [ -f "$file" ] || return 0
  grep -qF '# >>> eldencode >>>' "$file" 2>/dev/null || return 0
  awk '
    /^# >>> eldencode >>>$/ { skip = 1; next }
    /^# <<< eldencode <<<$/ { skip = 0; next }
    skip != 1 { print }
  ' "$file" >"$file.eldencode.tmp"
  mv "$file.eldencode.tmp" "$file"
  [ -n "$quiet" ] || echo "  path   removed block from $file"
}

add_path_block() {
  local file="$1"
  local updated=0
  if grep -qF '# >>> eldencode >>>' "$file" 2>/dev/null; then
    remove_path_block "$file" quiet
    updated=1
  fi
  {
    printf '\n# >>> eldencode >>>\n'
    printf '# added by the eldencode installer; remove with: %s/install.sh --uninstall\n' "$SELF_DIR"
    printf 'case ":$PATH:" in *":%s/bin:"*) ;; *) export PATH="%s/bin:$PATH" ;; esac\n' "$ELDENCODE_HOME" "$ELDENCODE_HOME"
    printf '# <<< eldencode <<<\n'
  } >>"$file"
  if [ "$updated" = 1 ]; then
    echo "  path   $file (updated)"
  else
    echo "  path   $file"
  fi
}

canonical_dir() {
  (cd -- "$1" 2>/dev/null && pwd -P)
}

# A purge must never be able to remove the user's home, an ancestor of it, the
# filesystem root, the source checkout, or an unrelated directory that merely
# happened to be passed as --home.
purge_guard() {
  local target canonical home source
  target="$1"
  [ -n "$target" ] || {
    echo "error: refusing to purge an empty path" >&2
    exit 2
  }
  [ -d "$target" ] || return 0
  canonical="$(canonical_dir "$target")" || {
    echo "error: cannot resolve $target" >&2
    exit 2
  }
  home="$(canonical_dir "$HOME")"
  source="$(canonical_dir "$SELF_DIR")"
  if [ "$canonical" = "/" ]; then
    echo "error: refusing to purge /" >&2
    exit 2
  fi
  if [ "$canonical" = "$home" ]; then
    echo "error: refusing to purge \$HOME ($canonical)" >&2
    exit 2
  fi
  case "$home/" in
    "$canonical/"*)
      echo "error: refusing to purge an ancestor of \$HOME ($canonical)" >&2
      exit 2
      ;;
  esac
  case "$source/" in
    "$canonical/"*)
      echo "error: refusing to purge the eldencode source checkout ($canonical)" >&2
      exit 2
      ;;
  esac
  if [ -f "$canonical/Cargo.toml" ]; then
    echo "error: refusing to purge a Rust source directory ($canonical)" >&2
    exit 2
  fi
  case "$(basename -- "$canonical")" in
    eldencode | .eldencode) return 0 ;;
    *) ;;
  esac
  # A custom --home is fine as long as the directory is unmistakably ours.
  if [ -x "$canonical/bin/eldencode" ] || [ -f "$canonical/config.toml" ] && [ -d "$canonical/backups" ]; then
    return 0
  fi
  echo "error: refusing to purge non-eldencode directory $canonical" >&2
  echo "       expected a directory named eldencode or .eldencode, or one" >&2
  echo "       created by this installer (bin/, config.toml, backups/)" >&2
  exit 2
}

if [ "$UNINSTALL" = 1 ]; then
  echo "uninstalling $APP"
  for rc in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
    remove_path_block "$rc"
  done
  if [ -f "$INSTALLED_BIN" ]; then
    rm -f -- "$INSTALLED_BIN"
    echo "  binary removed from $ELDENCODE_HOME/bin"
  fi
  if [ -L "$LOCAL_LINK" ] && [ "$(readlink "$LOCAL_LINK")" = "$INSTALLED_BIN" ]; then
    rm -f -- "$LOCAL_LINK"
    echo "  link   removed $LOCAL_LINK"
  fi
  if [ "$PURGE" = 1 ]; then
    purge_guard "$ELDENCODE_HOME"
    if [ -d "$ELDENCODE_HOME" ]; then
      rm -rf -- "$ELDENCODE_HOME"
      echo "  purged $ELDENCODE_HOME"
    fi
  else
    echo "  kept   config and save backups in $ELDENCODE_HOME"
  fi
  echo "done — open a new shell to refresh PATH"
  exit 0
fi

echo "installing $APP into $ELDENCODE_HOME"

if [ "$DO_BUILD" = 1 ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust from https://rustup.rs" >&2
    exit 1
  fi
  echo "  clean  cargo clean -p eldencode --release"
  (cd "$SELF_DIR" && cargo clean -p eldencode --release)
  echo "  build  cargo build --release (fresh)"
  (cd "$SELF_DIR" && cargo build --release)
fi

BIN_SRC="$SELF_DIR/target/release/eldencode"
if [ ! -x "$BIN_SRC" ]; then
  echo "error: missing $BIN_SRC (run again without --no-build)" >&2
  exit 1
fi

mkdir -p "$ELDENCODE_HOME/bin" "$ELDENCODE_HOME/backups"
install -m 0755 "$BIN_SRC" "$INSTALLED_BIN"
echo "  binary $INSTALLED_BIN"

if [ ! -f "$ELDENCODE_HOME/config.toml" ]; then
  cat >"$ELDENCODE_HOME/config.toml" <<EOF
# eldencode configuration — generated by install.sh
theme = "erdtree"
animations = true
# The Windows exe name Proton runs; --scan prints what matches right now.
process = "eldenring.exe"
auto_attach = true
# Copy ER0000.sl2 into backups/ every time the game is attached.
backup_on_attach = true
EOF
  echo "  config $ELDENCODE_HOME/config.toml"
else
  echo "  keep   $ELDENCODE_HOME/config.toml"
fi

while IFS= read -r rc; do
  add_path_block "$rc"
done < <(rc_files)

LOCAL_ON_PATH=0
case ":$PATH:" in
  *":$LOCAL_BIN:"*) LOCAL_ON_PATH=1 ;;
esac

if [ -e "$LOCAL_LINK" ] || [ -L "$LOCAL_LINK" ]; then
  echo "  keep   $LOCAL_LINK (left untouched)"
elif [ "$LOCAL_ON_PATH" = 1 ] || [ -d "$LOCAL_BIN" ]; then
  mkdir -p "$LOCAL_BIN"
  ln -s "$INSTALLED_BIN" "$LOCAL_LINK"
  echo "  link   $LOCAL_LINK"
else
  echo "  skip   $LOCAL_LINK ($LOCAL_BIN is not on PATH)"
fi

if [ -n "$LINK_DIR" ] && [ "$LINK_DIR" != "$LOCAL_BIN" ]; then
  if [ -e "$LINK_DIR/eldencode" ] || [ -L "$LINK_DIR/eldencode" ]; then
    echo "  keep   $LINK_DIR/eldencode (left untouched)"
  else
    mkdir -p "$LINK_DIR"
    ln -s "$INSTALLED_BIN" "$LINK_DIR/eldencode"
    echo "  link   $LINK_DIR/eldencode"
  fi
fi

if "$INSTALLED_BIN" --version >/dev/null 2>&1; then
  echo "  check  $("$INSTALLED_BIN" --version)"
else
  echo "  check  warning: the binary did not run" >&2
fi

FOUND="$(command -v eldencode 2>/dev/null || true)"
RUN_NOW="$INSTALLED_BIN"
RUN_NOTE=""
if [ -n "$FOUND" ] && [ "$FOUND" -ef "$INSTALLED_BIN" ]; then
  RUN_NOW="eldencode"
elif [ -e "$LOCAL_LINK" ] || [ -L "$LOCAL_LINK" ]; then
  if [ "$(readlink "$LOCAL_LINK" 2>/dev/null || true)" != "$INSTALLED_BIN" ]; then
    RUN_NOTE="$LOCAL_LINK already existed and was left untouched"
  else
    RUN_NOTE="open a new shell to pick up $LOCAL_LINK"
  fi
elif [ "$LOCAL_ON_PATH" = 0 ]; then
  RUN_NOTE="$LOCAL_BIN is not on PATH; use the full path in this shell"
else
  RUN_NOTE="open a new shell to pick up $LOCAL_LINK"
fi

cat <<EOF

eldencode is installed.

  run it now     $RUN_NOW
  binary         $INSTALLED_BIN
  first step     start Elden Ring with EAC disabled, press F5, then 2 for runes
EOF
if [ -n "$RUN_NOTE" ]; then
  printf '  note           %s\n' "$RUN_NOTE"
fi
cat <<EOF

  config         $ELDENCODE_HOME/config.toml
  backups        $ELDENCODE_HOME/backups/
  diagnostics    eldencode --scan

Offsets follow the Hexinton All in One 8.0.4 table (game 1.17.1).
Back up your save (b) before editing, and stay offline.
EOF
