#!/bin/bash

# Check for mprisence.service from systemctl
# If it exists, restart it
# If it doesn't exist, ask to create it
# 	by downloading it from https://raw.githubusercontent.com/phusitsom/mprisence/main/systemd/mprisence-local.service
# 		and placing it in $HOME/.config/systemd/user/mprisence.service
if [ -f "$HOME/.config/systemd/user/mprisence.service" ]; then
	# Check for symlink at /usr/local/bin/mprisence
	# If it exists, use it
	# If it doesn't exist, create it
	if [ -f "/usr/local/bin/mprisence" ]; then
		echo "mprisence symlink exists"
	else
		echo "mprisence symlink does not exist"
		echo "Creating mprisence symlink"
		if sudo ln -s "$(which mprisence)" /usr/local/bin/mprisence; then
			echo "mprisence symlink created"
		else
			echo "mprisence symlink could not be created"
			echo "Please create the symlink manually"
			echo "sudo ln -s $(which mprisence) /usr/local/bin/mprisence"
			exit
		fi
	fi

	mkdir -p "$HOME/.config/systemd/user"
	curl https://raw.githubusercontent.com/phusitsom/mprisence/main/systemd/mprisence-local.service >"$HOME/.config/systemd/user/mprisence.service"

	systemctl --user daemon-reload
	systemctl --user enable mprisence.service
	systemctl --user start mprisence.service
fi
