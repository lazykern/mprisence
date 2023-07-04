#!/bin/sh

# Check for mprisence.service from systemctl
# If it exists, restart it
# If it doesn't exist, ask to create it
# 	by downloading it from https://raw.githubusercontent.com/phusitsom/mprisence/main/systemd/mprisence-local.service
# 		and placing it in $HOME/.config/systemd/user/mprisence.service
if [ -f "$HOME/.config/systemd/user/mprisence.service" ]; then
	echo "mprisence.service exists"
	echo "Restarting mprisence.service"
	systemctl --user restart mprisence.service
else
	echo "mprisence.service does not exist"
	echo "Create and start mprisence.service? (Y/n)"
	read -r create_mprisence_service
	if [ "$create_mprisence_service" != "n" ]; then

		# Check for symlink at /usr/local/bin/mprisence
		# If it exists, use it
		# If it doesn't exist, create it
		if [ -f "/usr/local/bin/mprisence" ]; then
			echo "mprisence symlink exists"
		else
			echo "mprisence symlink does not exist"
			echo "Creating mprisence symlink"
			if sudo ln -s "$HOME/.cargo/bin/mprisence" /usr/local/bin/mprisence; then
				echo "mprisence symlink created"
			else
				echo "mprisence symlink could not be created"
				echo "Please create the symlink manually"
				exit
			fi
		fi

		mkdir -p "$HOME/.config/systemd/user"
		curl https://raw.githubusercontent.com/phusitsom/mprisence/main/systemd/mprisence-local.service >"$HOME/.config/systemd/user/mprisence.service"
		systemctl --user daemon-reload
		systemctl --user enable mprisence.service
		systemctl --user start mprisence.service
	fi
fi
