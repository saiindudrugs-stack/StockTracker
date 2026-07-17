#!/bin/bash
# Given a crate name, tries progressively older patch/minor versions until
# `cargo fetch` stops failing on it specifically, then appends a working pin
# to apps/desktop/src-tauri/Cargo.toml.
set -e
CRATE="$1"
shift
VERSIONS=("$@")
TOML=apps/desktop/src-tauri/Cargo.toml

for v in "${VERSIONS[@]}"; do
  cp "$TOML" /tmp/toml_backup.toml
  echo "${CRATE} = \"=${v}\"" >> "$TOML"
  rm -f Cargo.lock
  if cargo generate-lockfile > /tmp/genlock.log 2>&1; then
    if cargo fetch > /tmp/fetch.log 2>&1; then
      echo "WORKS: ${CRATE} ${v}"
      exit 0
    fi
  fi
  cp /tmp/toml_backup.toml "$TOML"
done
echo "NONE WORKED for ${CRATE}"
tail -20 /tmp/fetch.log 2>/dev/null || tail -20 /tmp/genlock.log
exit 1
