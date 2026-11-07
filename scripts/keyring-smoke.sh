#!/usr/bin/env bash
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
YIGOU_KEYRING_TEST="$YIGOU_ROOT/builds/keyring-smoke"
mkdir -p "$YIGOU_KEYRING_TEST"/{data,config,control}
chmod 700 "$YIGOU_KEYRING_TEST" "$YIGOU_KEYRING_TEST"/{data,config,control}
export XDG_DATA_HOME="$YIGOU_KEYRING_TEST/data"
export XDG_CONFIG_HOME="$YIGOU_KEYRING_TEST/config"
export GNOME_KEYRING_CONTROL="$YIGOU_KEYRING_TEST/control"
export YIGOU_KEYRING_TEST
# A private D-Bus session and temporary keyring; never accesses a user's wallet.
exec dbus-run-session -- bash -euo pipefail -c '
  trap '\''if [[ -n "${yigou_keyring_pid:-}" ]]; then kill "$yigou_keyring_pid" 2>/dev/null || true; wait "$yigou_keyring_pid" 2>/dev/null || true; fi'\'' EXIT
  printf "%s" "fixture-keyring-password" | "$YIGOU_ROOT/builds/sysroot/usr/bin/gnome-keyring-daemon" --foreground --unlock --components=secrets --control-directory="$GNOME_KEYRING_CONTROL" > "$YIGOU_KEYRING_TEST/daemon.log" 2>&1 &
  yigou_keyring_pid=$!
  for attempt in {1..30}; do
    if gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.NameHasOwner org.freedesktop.secrets | rg -q true; then
      "$YIGOU_ROOT/builds/target/debug/examples/vault_probe"
      exit 0
    fi
    sleep 0.2
  done
  exit 1
'
