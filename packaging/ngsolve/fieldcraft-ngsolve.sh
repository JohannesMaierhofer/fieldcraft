#!/bin/sh
set -eu
root=/opt/fieldcraft-ngsolve
exec "$root/fieldcraft-cli" ngsolve "$@" --python "$root/python/bin/python3"
