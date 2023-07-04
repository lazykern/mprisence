#!/bin/sh

TEMP_DIR="$(mktemp -d)"
curl https://raw.githubusercontent.com/phusitsom/mprisence/main/scripts/helper/example-config.sh >"$TEMP_DIR/example-config.sh"
chmod +x "$TEMP_DIR/example-config.sh"
"$TEMP_DIR/example-config.sh"
