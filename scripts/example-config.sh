#!/bin/sh

CONFIG_URL=https://raw.githubusercontent.com/phusitsom/mprisence/main/config/example.toml
CONFIG_PATH="${XDG_CONFIG_HOME:-$HOME/.config}/mprisence/config.toml"

create_config() {
	curl "$CONFIG_URL" >"$CONFIG_PATH"
	echo "mprisence config created at $CONFIG_PATH"
}

if [ -f "$CONFIG_PATH" ]; then
	echo "mprisence config exists"
	echo "do you want to overwrite it? (y/N)"
	echo "backup will be created at $CONFIG_PATH.bak"
	read -r overwrite_config
	if [ "$overwrite_config" = "y" ]; then
		cp "$CONFIG_PATH" "$CONFIG_PATH.bak"
		create_config
	fi
else
	mkdir -p "$(dirname "$CONFIG_PATH")"
	create_config
fi
