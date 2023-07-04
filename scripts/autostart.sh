#!/bin/sh

TEMP_DIR="$(mktemp -d)"

curl https://raw.githubusercontent.com/phusitsom/mprisence/main/scripts/autostart.sh >"$TEMP_DIR/autostart.sh"
chmod +x "$TEMP_DIR/autostart.sh"
"$TEMP_DIR/autostart.sh"
