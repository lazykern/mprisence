#!/bin/sh

# Ask to install cargo if not installed
if ! command -v cargo; then
	echo "cargo could not be found"
	echo "Install cargo? (Y/n)"
	read -r install_cargo
	if [ "$install_cargo" != "n" ]; then
		curl https://sh.rustup.rs -sSf | sh
	else
		echo "cargo is required to install the program"
		exit
	fi
fi

# git
if ! command -v git; then
	echo "git could not be found"
	echo "Install git? (Y/n)"
	read -r install_git
	if [ "$install_git" != "n" ]; then
		sudo apt install git
	else
		echo "git is required to install the program"
		exit
	fi
fi

# build-essentials
if ! command -v make; then
	echo "build-essentials could not be found"
	echo "Install build-essentials? (Y/n)"
	read -r install_build_essentials
	if [ "$install_build_essentials" != "n" ]; then
		if [ -f /etc/debian_version ]; then
			sudo apt install build-essential
		elif [ -f /etc/arch-release ]; then
			sudo pacman -S base-devel
		elif [ -f /etc/redhat-release ]; then
			sudo yum groupinstall 'Development Tools'
		else
			echo "Please install build-essentials on your system"
		fi
	else
		echo "build-essentials is required to install the program"
		exit
	fi
fi

# libssl-dev
if ! command -v openssl; then
	echo "libssl-dev could not be found"
	echo "Install libssl-dev? (Y/n)"
	read -r install_libssl_dev
	if [ "$install_libssl_dev" != "n" ]; then
		if [ -f /etc/debian_version ]; then
			sudo apt install libssl-dev
		elif [ -f /etc/arch-release ]; then
			sudo pacman -S openssl
		elif [ -f /etc/redhat-release ]; then
			sudo yum install openssl-devel
		else
			echo "Please install libssl-dev on your system"
		fi
	else
		echo "libssl-dev is required to install the program"
		exit
	fi
fi

# libdbus-1-dev
if ! command -v dbus-daemon; then
	echo "libdbus-1-dev could not be found"
	echo "Install libdbus-1-dev? (Y/n)"
	read -r install_libdbus_1_dev
	if [ "$install_libdbus_1_dev" != "n" ]; then
		if [ -f /etc/debian_version ]; then
			sudo apt install libdbus-1-dev
		elif [ -f /etc/arch-release ]; then
			sudo pacman -S dbus
		elif [ -f /etc/redhat-release ]; then
			sudo yum install dbus-devel
		else
			echo "Please install libdbus-1-dev on your system"
		fi
	else
		echo "libdbus-1-dev is required to install the program"
		exit
	fi
fi

# pkg-config
if ! command -v pkg-config; then
	echo "pkg-config could not be found"
	echo "Install pkg-config? (Y/n)"
	read -r install_pkg_config
	if [ "$install_pkg_config" != "n" ]; then
		if [ -f /etc/debian_version ]; then
			sudo apt install pkg-config
		elif [ -f /etc/arch-release ]; then
			sudo pacman -S pkgconf
		elif [ -f /etc/redhat-release ]; then
			sudo yum install pkgconfig
		else
			echo "Please install pkg-config on your system"
		fi
	else
		echo "pkg-config is required to install the program"
		exit
	fi
fi

echo "Installing mprisence..."

cargo install --git https://github.com/phusitsom/mprisence --branch main

echo "Do you want to set up a systemd service (autostart) for mprisence? (Y/n)"
read -r install_service
if [ "$install_service" != "n" ]; then
	curl "https://github.com/phusitsom/mprisence/scritps/autostart.sh" | sh
fi

echo "mprisence has been installed"
