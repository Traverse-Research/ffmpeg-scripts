#!/bin/bash
# Detect scene changes / cut points in a video
# Usage: ./detect-cuts.sh <video_file> [threshold]
#
# threshold: 0.0-1.0, lower = more sensitive (default: 0.3)
# Output: timestamps where scene changes occur

VIDEO="${1:?Usage: $0 <video_file> [threshold]}"
THRESHOLD="${2:-0.3}"

if [ ! -f "$VIDEO" ]; then
    echo "File not found: $VIDEO"
    exit 1
fi

echo "Analyzing: $VIDEO"
echo "Threshold: $THRESHOLD (lower = more sensitive)"
echo ""
echo "Detecting scene changes..."
echo ""

# Use FFmpeg's select filter with scene detection
# This outputs timestamps where scene change score exceeds threshold
ffmpeg -i "$VIDEO" -vf "select='gt(scene,$THRESHOLD)',showinfo" -vsync vfr -f null - 2>&1 | \
    grep showinfo | \
    sed -n 's/.*pts_time:\([0-9.]*\).*/\1/p' | \
    while read -r timestamp; do
        # Convert to HH:MM:SS format
        hours=$(echo "$timestamp / 3600" | bc)
        mins=$(echo "($timestamp % 3600) / 60" | bc)
        secs=$(echo "$timestamp % 60" | bc)
        printf "%02d:%02d:%05.2f\n" "$hours" "$mins" "$secs"
    done

echo ""
echo "Done!"
