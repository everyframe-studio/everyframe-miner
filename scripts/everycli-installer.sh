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
  # Release binaries are Ed25519-signed (see RELEASING.md). Raw key hex:
  # a81643f1026fb20e85399c23fc804fee82bc2e58e570efeaf30a6571eab04149
  release_key='-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAqBZD8QJvsg6FOZwj/IBP7oK8LljlcO/q8wplceqwQUk=
-----END PUBLIC KEY-----'
  # Known-answer vector (throwaway key) proving an openssl can verify Ed25519.
  self_test_key='-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAWzasPwpou/SgbOksNov3B5nCUrXQ2UVyeoi0ZQAeBwM=
-----END PUBLIC KEY-----'
  self_test_sig='E1g8VL6EM+BKVjotJSzyADPL2qD1TZ7PBm8r0ZnCJMnXOAESBoOfZ7KrDUwiBhPlTrpmUKAAAmJAMninFShJBA=='
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
    rm -f "$lock_dir/everycli" "$lock_dir/checksum" "$lock_dir/version" "$lock_dir/signature" \
      "$lock_dir/signature.bin" "$lock_dir/message" "$lock_dir/release.pem" \
      "$lock_dir/self-test.pem" "$lock_dir/self-test.msg" "$lock_dir/self-test.sig"
    rmdir "$lock_dir" 2>/dev/null || :
  }
  trap cleanup EXIT
  trap 'exit 1' HUP INT TERM
  # Ed25519 verification needs OpenSSL 1.1.1+ (`pkeyutl -rawin`); macOS's
  # LibreSSL cannot, so Homebrew's keg-only OpenSSL is also tried. Fail closed.
  openssl_works() {
    printf '%s\n' "$self_test_key" > "$lock_dir/self-test.pem" &&
      printf 'everycli-openssl-self-test-v1\n' > "$lock_dir/self-test.msg" &&
      printf '%s' "$self_test_sig" | "$1" base64 -d -A > "$lock_dir/self-test.sig" 2>/dev/null &&
      "$1" pkeyutl -verify -pubin -inkey "$lock_dir/self-test.pem" -rawin \
        -in "$lock_dir/self-test.msg" -sigfile "$lock_dir/self-test.sig" >/dev/null 2>&1
  }
  openssl_bin=''
  for candidate in "$(command -v openssl 2>/dev/null || :)" /opt/homebrew/opt/openssl@3/bin/openssl /usr/local/opt/openssl@3/bin/openssl; do
    if [ -n "$candidate" ] && [ -x "$candidate" ] && openssl_works "$candidate"; then
      openssl_bin=$candidate
      break
    fi
  done
  [ -n "$openssl_bin" ] || fail 'OpenSSL 1.1.1 or newer with Ed25519 support is required to verify the release signature (macOS: brew install openssl@3). Nothing was installed.'
  asset="everycli-$target"
  base="https://github.com/everyframe-studio/everyframe-miner/releases/download/$version"
  download() {
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fLsS --connect-timeout 30 --max-time 180 --retry 3 --retry-delay 2 --retry-max-time 240 --retry-connrefused --max-redirs 5 --max-filesize 67108864 "$1" -o "$2"
  }
  printf 'Installing everycli %s for %s…\n' "$version" "$target"
  download "$base/$asset.sha256" "$lock_dir/checksum" || fail 'Could not download release checksum. No binary was replaced.'
  download "$base/$asset.sig" "$lock_dir/signature" || fail 'Could not download the release signature; refusing to install an unsigned binary. No binary was replaced.'
  download "$base/$asset" "$lock_dir/everycli" || fail 'Could not download release binary. No binary was replaced.'
  expected=$(awk -v name="$asset" 'NF == 2 && $2 == name {print $1}' "$lock_dir/checksum")
  [ "${#expected}" -eq 64 ] || fail 'Invalid release checksum.'
  case "$expected" in *[!0-9a-fA-F]*) fail 'Invalid release checksum.' ;; esac
  if [ "$checksum_tool" = sha256sum ]; then
    actual=$(sha256sum "$lock_dir/everycli" | awk '{print $1}')
  else
    actual=$(shasum -a 256 "$lock_dir/everycli" | awk '{print $1}')
  fi
  actual=$(printf '%s' "$actual" | tr 'ABCDEF' 'abcdef')
  [ "$actual" = "$(printf '%s' "$expected" | tr 'ABCDEF' 'abcdef')" ] || fail 'Checksum mismatch. No binary was replaced.'
  # Verify the publisher signature before the downloaded binary is ever run.
  signature=$(tr -d ' \t\r\n' < "$lock_dir/signature")
  [ "${#signature}" -eq 88 ] || fail 'Invalid release signature. No binary was replaced.'
  case "$signature" in *[!A-Za-z0-9+/=]*) fail 'Invalid release signature. No binary was replaced.' ;; esac
  printf '%s' "$signature" | "$openssl_bin" base64 -d -A > "$lock_dir/signature.bin" 2>/dev/null || fail 'Invalid release signature. No binary was replaced.'
  [ "$(wc -c < "$lock_dir/signature.bin" | tr -d ' ')" = 64 ] || fail 'Invalid release signature. No binary was replaced.'
  printf 'everycli-release-v1\n%s\n%s\n%s\n' "$version" "$asset" "$actual" > "$lock_dir/message"
  printf '%s\n' "$release_key" > "$lock_dir/release.pem"
  "$openssl_bin" pkeyutl -verify -pubin -inkey "$lock_dir/release.pem" -rawin \
    -in "$lock_dir/message" -sigfile "$lock_dir/signature.bin" >/dev/null 2>&1 ||
    fail 'Release signature verification failed: this binary was not signed by the EveryFrame release key for this version. No binary was replaced.'
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
  printf 'Then run: everycli --version\nUpgrade later: everycli update\nAutomatic CLI updates: everycli auto-update (disable with --disable). Workers are unchanged.\n'
)
everycli_install "$@"
