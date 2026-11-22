#!/bin/sh
set -eu
unset PYTHONHOME PYTHONPATH
root=/opt/fieldcraft-ngsolve
export PYTHONNOUSERSITE=1
exec "$root/python/bin/python3" "$root/bridge/workflow.py" "$@"
