#!/bin/bash

if ! command -v mprisence >/dev/null; then
	echo "mprisence is not installed"
	echo "exiting..."
	exit
fi

if [ -f "/usr/local/bin/mprisence" ]; then
	echo "mprisence symlink exists"
else
	echo "mprisence symlink does not exist"
	echo "Creating mprisence symlink"
	if sudo ln -s "$(which mprisence)" "/usr/local/bin/mprisence"; then
		echo "mprisence symlink created"
	else
		echo "mprisence symlink could not be created"
		echo "Please create the symlink manually"
		echo "sudo ln -s $(which mprisence) /usr/local/bin/mprisence"
		exit
	fi
fi

echo "Creating systemd service at $HOME/.config/systemd/user/mprisence.service"
mkdir -p "$HOME/.config/systemd/user"
curl https://raw.githubusercontent.com/pullinglazy/mprisence/main/systemd/mprisence-local.service >"$HOME/.config/systemd/user/mprisence.service"

echo "Enabling and starting systemd service"
systemctl --user daemon-reload
systemctl --user enable mprisence.service
systemctl --user start mprisence.service
