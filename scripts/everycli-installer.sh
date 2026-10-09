#!/bin/sh
# Apache-2.0. Releases replace this marker with their immutable version tag.
# The complete function is parsed before any installation runs (safe for curl | sh).
everycli_install() (
  set -eu
  umask 077
  version='@VERSION@'
  fail() { printf 'everycli: %s\n' "$*" >&2; exit 1; }
  [ "$#" -eq 0 ] || fail 'This installer takes no arguments. Use a versioned release URL.'
  printf '%s\n' "$version" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || fail 'Use the installer from a published GitHub release, not the source template.'
  command -v curl >/dev/null 2>&1 || fail 'curl is required.'
  case "$(uname -s)/$(uname -m)" in
    Linux/x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-gnu ;;
    Darwin/x86_64) target=x86_64-apple-darwin ;;
    Darwin/arm64) target=aarch64-apple-darwin ;;
    *) fail 'Supported platforms: Linux and macOS, x86_64 and ARM64. Use WSL on Windows.' ;;
  esac
  if command -v sha256sum >/dev/null 2>&1; then
    checksum_tool=sha256sum
  elif command -v shasum >/dev/null 2>&1; then
    checksum_tool=shasum
  else
    fail 'sha256sum or shasum is required.'
  fi
  install_root=${CARGO_HOME:-"$HOME/.cargo"}
  case "$install_root" in /*) ;; *) fail 'CARGO_HOME must be an absolute path.' ;; esac
  case "$install_root" in *'
'*) fail 'Installation paths cannot contain newlines.' ;; esac
  install_dir="$install_root/bin"
  mkdir -p "$install_dir"
  [ ! -L "$install_dir/everycli" ] || fail 'Refusing to replace an everycli symlink.'
  [ ! -e "$install_dir/everycli" ] || [ -f "$install_dir/everycli" ] || fail 'The destination is not a regular file.'
  lock_dir="$install_dir/.everycli-update.lock"
  mkdir "$lock_dir" 2>/dev/null || fail 'Another install/update is running, or the directory is not writable.'
  cleanup() {
    rm -f "$lock_dir/everycli" "$lock_dir/checksum" "$lock_dir/version"
    rmdir "$lock_dir" 2>/dev/null || :
  }
  trap cleanup EXIT
  trap 'exit 1' HUP INT TERM
  asset="everycli-$target"
  base="https://github.com/everyframe-studios/everyframe-miner/releases/download/$version"
  download() {
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fLsS --connect-timeout 30 --max-time 180 --retry 3 --retry-delay 2 --retry-max-time 240 --retry-connrefused --max-redirs 5 --max-filesize 67108864 "$1" -o "$2"
  }
  printf 'Installing everycli %s for %s…\n' "$version" "$target"
  download "$base/$asset.sha256" "$lock_dir/checksum" || fail 'Could not download release checksum. No binary was replaced.'
  download "$base/$asset" "$lock_dir/everycli" || fail 'Could not download release binary. No binary was replaced.'
  expected=$(awk -v name="$asset" 'NF == 2 && $2 == name {print $1}' "$lock_dir/checksum")
  [ "${#expected}" -eq 64 ] || fail 'Invalid release checksum.'
  case "$expected" in *[!0-9a-fA-F]*) fail 'Invalid release checksum.' ;; esac
  if [ "$checksum_tool" = sha256sum ]; then
    actual=$(sha256sum "$lock_dir/everycli" | awk '{print $1}')
  else
    actual=$(shasum -a 256 "$lock_dir/everycli" | awk '{print $1}')
  fi
  [ "$actual" = "$expected" ] || fail 'Checksum mismatch. No binary was replaced.'
  chmod 755 "$lock_dir/everycli"
  "$lock_dir/everycli" --version > "$lock_dir/version" || fail 'Binary cannot run on this system. No binary was replaced.'
  [ "$(cat "$lock_dir/version")" = "everycli ${version#v} (Rust)" ] || fail 'Release version mismatch. No binary was replaced.'
  mv -f "$lock_dir/everycli" "$install_dir/everycli"
  printf 'Installed: %s/everycli\n' "$install_dir"
  # Single-quote arbitrary paths safely before writing shell configuration.
  quote() { printf "'"; printf '%s' "$1" | sed "s/'/'\\\\''/g"; printf "'"; }
  if [ "${EVERYCLI_NO_MODIFY_PATH:-0}" != 1 ]; then
    env_file="$install_root/everycli-env"
    [ ! -L "$env_file" ] || fail 'Refusing to overwrite a symlinked PATH configuration.'
    quoted_bin=$(quote "$install_dir")
    printf 'case ":${PATH}:" in\n  *:%s:*) ;;\n  *) export PATH=%s:"$PATH" ;;\nesac\n' "$quoted_bin" "$quoted_bin" > "$env_file"
    source_line=". $(quote "$env_file")"
    for profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
      if [ -L "$profile" ]; then
        printf 'Skipped symlinked shell profile: %s\n' "$profile" >&2
      elif ! grep -Fqx "$source_line" "$profile" 2>/dev/null; then
        printf '\n# EveryFrame CLI\n%s\n' "$source_line" >> "$profile"
      fi
    done
    case ":${PATH}:" in
      *:"$install_dir":*) ;; # Already usable in the calling shell.
      *) printf 'To use everycli in this terminal, run:\n  %s\nOr open a new terminal.\n' "$source_line" ;;
    esac
  else
    case ":${PATH}:" in
      *:"$install_dir":*) ;;
      *) printf 'PATH was not modified. Add %s to PATH.\n' "$install_dir" ;;
    esac
  fi
  printf 'Then run: everycli --version\nUpgrade later: everycli update\n'
)
everycli_install "$@"
