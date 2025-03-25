#!/bin/sh

# Use POSIX shell for better compatibility
set -e  # Exit on error

# Check if systemd is available
if ! command -v systemctl >/dev/null 2>&1; then
    echo "Error: systemd is not available on this system"
    exit 1
fi

# Check if running as root (which we don't want)
if [ "$(id -u)" = "0" ]; then
    echo "Error: This script should not be run as root"
    echo "Please run it as your normal user"
    exit 1
fi

# Get the directory where the script is located
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Create systemd user directory if it doesn't exist
mkdir -p "${HOME}/.config/systemd/user/"

echo "Installing systemd units..."

# Copy service file
if [ -f "${SCRIPT_DIR}/../mprisence.service" ]; then
    cp "${SCRIPT_DIR}/../mprisence.service" "${HOME}/.config/systemd/user/"
else
    echo "Error: Service file not found in ${SCRIPT_DIR}"
    echo "Please run this script from the contrib/systemd directory"
    exit 1
fi

# Check if mprisence is in PATH
if ! command -v mprisence >/dev/null 2>&1; then
    echo "Warning: mprisence not found in PATH"
    echo "Make sure mprisence is installed and available in your PATH"
    echo "The service will not work until mprisence is properly installed"
fi

echo "Reloading systemd daemon..."
systemctl --user daemon-reload || {
    echo "Error: Failed to reload systemd daemon"
    echo "Make sure your user session has systemd user instance running"
    exit 1
}

echo "Stopping any existing mprisence service..."
systemctl --user stop mprisence.service 2>/dev/null || true

echo "Enabling mprisence units..."

if ! systemctl --user enable --no-block mprisence.service; then
    echo "Error: Failed to enable mprisence service"
    exit 1
fi

echo
echo "Installation complete!"
echo "mprisence has been installed and will automatically:"
echo "  - Start when Discord is running (detected via SingletonLock file)"
echo "  - Stop when Discord is closed"
echo
echo "To check the service status:"
echo "  systemctl --user status mprisence.service"
echo
echo "To view logs:"
echo "  journalctl --user -u mprisence"
echo
echo "To uninstall:"
echo "  systemctl --user disable --now mprisence.service"
echo "  rm ~/.config/systemd/user/mprisence.service" 