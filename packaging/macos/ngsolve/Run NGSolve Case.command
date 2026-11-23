#!/bin/zsh
# Finder starts this command from the Applications directory of the installation.
set -eu
if (( $# > 1 )); then
    print -u2 'Usage: Run NGSolve Case.command [case.json]'
    exit 2
fi
if (( $# == 1 )); then
    case_file=$1
else
    case_file=$(/usr/bin/osascript <<'APPLESCRIPT'
try
    return POSIX path of (choose file with prompt "Select your FieldCraft NGSolve case (.json)" of type {"public.json"})
on error number -128
    return ""
end try
APPLESCRIPT
)
    [[ -n "$case_file" ]] || exit 0
fi
[[ -f "$case_file" ]] || { print -u2 "Case file not found: $case_file"; exit 1; }
case_file=${case_file:A}
apps=${0:a:h}
prefix=${apps:h}
solver="${prefix%/}/Library/Application Support/FieldCraft/NGSolve/fieldcraft-ngsolve"
app="$apps/FieldCraft.app"
if [[ "$apps" == "$HOME/Applications" && -d "$apps/FieldCraft Intel.app" ]]; then
    app="$apps/FieldCraft Intel.app"
fi
gui="$app/Contents/MacOS/FieldCraft"
[[ -x "$solver" && -x "$gui" ]] || { print -u2 "The FieldCraft app and NGSolve runtime must be installed under $prefix."; exit 1; }

runs="${case_file:h}/calculations"
mkdir -p "$runs"
output="$runs/$(/bin/date -u +%Y%m%dT%H%M%SZ)-$(/usr/bin/uuidgen)"
log="$output.log"
print -r -- "Case: $case_file"
print -r -- "Log: $log"
print -r -- "Running NGSolve. FieldCraft will open the results when the calculation succeeds."
if "$solver" case "$case_file" "$output" --gui "$gui" >"$log" 2>&1; then
    print -r -- "Results: $output/results.json"
else
    code=$?
    if [[ -f "$output/results.json" ]]; then
        print -u2 -r -- "The solver saved its results in $output."
    fi
    /usr/bin/tail -n 35 "$log" >&2
    print -u2 -r -- "The case command exited with status $code. See $log"
    exit "$code"
fi
