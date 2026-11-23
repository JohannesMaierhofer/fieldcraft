#!/bin/sh
# Run as an ordinary user with the GUI launcher supplied as the first argument.
set -eu
launcher=$(realpath "${1:-/usr/bin/fieldcraft-ngsolve-gui}")
[ "$(id -u)" != 0 ] || { echo 'Run this profile check as a non-root user' >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
export HOME="$work/home" TMPDIR="$work/tmp"
unset FIELDCRAFT_RECOVERY_DIR FIELDCRAFT_CONTROL_PORT XDG_DATA_HOME CI
mkdir -p "$HOME" "$TMPDIR"
initial='{"show_donate_button":false,"update_check":false}'
retained='{"update_check":true,"show_donate_button":true,"extra":"keep"}'
launch() { "$launcher" "$@" --version > /dev/null; }
fresh() {
    [ "$(cat "$1")" = "$initial" ]
    [ "$(stat -c %a "$1")" = 600 ]
}
homefile=$HOME/.local/share/fieldcraft/prefs.json
auto=$TMPDIR/fieldcraft-automation/prefs.json
# Automation has no effect on a fresh production profile.
CI=true launch
fresh "$auto"; [ ! -e "$homefile" ]
rm "$auto"
FIELDCRAFT_CONTROL_PORT=' +00023 ' launch
fresh "$auto"; [ ! -e "$homefile" ]
rm "$auto"
launch --control 0
fresh "$auto"; [ ! -e "$homefile" ]
FIELDCRAFT_CONTROL_PORT='65536' launch
fresh "$homefile"
printf '%s\n' "$retained" > "$homefile"
launch
[ "$(cat "$homefile")" = "$retained" ]
export XDG_DATA_HOME="$work/xdg data"
launch
fresh "$XDG_DATA_HOME/fieldcraft/prefs.json"
export FIELDCRAFT_RECOVERY_DIR="$work/custom area/recovery/"
a=0
pids=''
while [ "$a" -lt 4 ]; do
    CI=true launch --control 0 &
    pids="$pids $!"
    a=$((a+1))
done
for pid in $pids; do wait "$pid"; done
custom="$work/custom area/prefs.json"
fresh "$custom"
rm "$custom"
ln -s "$work/missing" "$custom"
launch
[ -L "$custom" ] && [ ! -e "$work/missing" ]
echo 'Verified HOME, XDG, explicit recovery, CI, control argument and environment, preserved user content and concurrent launches.'
