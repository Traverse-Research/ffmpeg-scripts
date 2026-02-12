#!/bin/bash
# Show a full-size preview of frame 0 of a video using img2sixel
# Usage: ./preview.sh <video_file>

VIDEO="${1:?Usage: $0 <video_file>}"

if [ ! -f "$VIDEO" ]; then
    echo "File not found: $VIDEO"
    exit 1
fi

tmpfile=$(mktemp /tmp/preview_XXXXXX.jpg)
trap "rm -f $tmpfile" EXIT

ffmpeg -y -ss 0 -i "$VIDEO" -vframes 1 -q:v 2 "$tmpfile" 2>/dev/null

img2sixel "$tmpfile"
