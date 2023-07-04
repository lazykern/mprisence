#!/bin/bash

# Disable systemd service
if systemctl --user --now disable mprisence.service &>/dev/null; then
	echo "mprisence.service disabled"
fi

# Remove systemd service
if rm ~/.config/systemd/user/mprisence.service &>/dev/null; then
	echo "mprisence.service removed"
fi

# Remove symlinks
if sudo rm /usr/local/bin/mprisence &>/dev/null; then
	echo "mprisence symlinks removed"
fi

# Remove mprisence
if cargo uninstall mprisence &>/dev/null; then
	echo "mprisence uninstalled with cargo"
fi
