#!/usr/bin/env bash
# Dump level oracle data from a ROM with Mesen 2.
# Usage: tools/oracle/dump.sh <rom> <outdir> <level>[,<level>...]   (levels in hex)
set -euo pipefail
rom=$1; out=$2; levels=$3
mesen=${MESEN:-$HOME/.local/share/kobo/tools/mesen/Mesen}
mkdir -p "$out"
# Mesen wants a .sfc/.smc extension; copy so it never touches the source ROM.
tmp=$(mktemp -d)
cp "$rom" "$tmp/rom.sfc"
export KOBO_ORACLE_OUT=$(realpath "$out") KOBO_ORACLE_LEVELS=$levels
export LC_ALL=C.UTF-8 DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1
script=$(realpath "$(dirname "$0")/dump_levels.lua")
set +e
"$mesen" --testRunner "$script" "$tmp/rom.sfc" --timeout="${KOBO_ORACLE_TIMEOUT:-900}" > "$out/mesen.stdout" 2>&1
code=$?
set -e
rm -rf "$tmp"
echo "mesen exit $code"
tail -n 5 "$out/oracle.log" 2>/dev/null || true
exit $code
