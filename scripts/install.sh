#!/bin/sh

TEMP_DIR="$(mktemp -d)"
curl https://raw.githubusercontent.com/phusitsom/mprisence/main/scripts/helper/install.sh >"$TEMP_DIR/install.sh"
chmod +x "$TEMP_DIR/install.sh"
"$TEMP_DIR/install.sh"
