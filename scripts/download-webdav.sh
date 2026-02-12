#!/bin/bash
# Download all files from a WebDAV server using rclone
# Usage: ./download-webdav.sh <webdav_url> <username> <password> [output_dir]
#
# Requires: rclone
# Install with: curl https://rclone.org/install.sh | sudo bash

set -e

WEBDAV_URL="${1:?Usage: $0 <webdav_url> <username> <password> [output_dir]}"
USERNAME="${2:?Username required}"
PASSWORD="${3:?Password required}"
OUTPUT_DIR="${4:-.}"

# Remove trailing slash from URL
WEBDAV_URL="${WEBDAV_URL%/}"

echo "Downloading from: $WEBDAV_URL"
echo "Output directory: $OUTPUT_DIR"

mkdir -p "$OUTPUT_DIR"

# Check if rclone is installed
if ! command -v rclone &> /dev/null; then
    echo "rclone not found. Install with: curl https://rclone.org/install.sh | sudo bash"
    exit 1
fi

# Use rclone with inline WebDAV config
# --webdav-url: WebDAV server URL
# --webdav-user: username
# --webdav-pass: password (obscured)
OBSCURED_PASS=$(rclone obscure "$PASSWORD")

rclone copy \
    --webdav-url="$WEBDAV_URL" \
    --webdav-user="$USERNAME" \
    --webdav-pass="$OBSCURED_PASS" \
    --progress \
    --transfers=4 \
    ":webdav:/" \
    "$OUTPUT_DIR"

echo "Download complete!"
