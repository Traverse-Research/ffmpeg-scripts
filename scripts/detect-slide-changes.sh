#!/bin/bash
# Detect major slide changes in the processed video's slide overlay area
# The slides appear as a 320px-height overlay in the bottom-right corner with 40px margin
# Usage: ./detect-slide-changes.sh <video_file> [threshold]
#
# threshold: 0.0-1.0, higher = only major changes (default: 0.3)

VIDEO="${1:?Usage: $0 <video_file> [threshold]}"
THRESHOLD="${2:-0.3}"

if [ ! -f "$VIDEO" ]; then
    echo "File not found: $VIDEO"
    exit 1
fi

# Get video dimensions
dimensions=$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0 "$VIDEO" 2>/dev/null)
width=$(echo "$dimensions" | cut -d',' -f1)
height=$(echo "$dimensions" | cut -d',' -f2)

# The slide overlay is 320px tall, aspect ratio ~16:9, so roughly 569x320
# Positioned at bottom-right with 40px margin
# crop=w:h:x:y
crop_w=569
crop_h=320
crop_x=$((width - crop_w - 40))
crop_y=$((height - crop_h - 40))

echo "========================================"
echo "Analyzing: $(basename "$VIDEO")"
echo "Video size: ${width}x${height}"
echo "Monitoring: Slide overlay area (${crop_w}x${crop_h} at ${crop_x},${crop_y})"
echo "Threshold: $THRESHOLD"
echo "========================================"
echo ""

# Get video duration
duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$VIDEO" 2>/dev/null)
echo "Video duration: $(printf '%02d:%02d:%02d' $((${duration%.*}/3600)) $((${duration%.*}%3600/60)) $((${duration%.*}%60)))"
echo ""
echo "Detecting slide changes (this may take a while)..."
echo ""

# Crop to the slide overlay area, then detect scene changes
ffmpeg -i "$VIDEO" \
    -vf "crop=${crop_w}:${crop_h}:${crop_x}:${crop_y},select='gt(scene,$THRESHOLD)',showinfo" \
    -vsync vfr -f null - 2>&1 | \
    grep showinfo | \
    sed -n 's/.*pts_time:\([0-9.]*\).*/\1/p' | \
    while read -r timestamp; do
        hours=$(echo "$timestamp / 3600" | bc)
        mins=$(echo "($timestamp % 3600) / 60" | bc)
        secs=$(echo "$timestamp % 60" | bc)
        printf "%02d:%02d:%05.2f\n" "$hours" "$mins" "$secs"
    done | tee /tmp/slide_changes.txt

count=$(wc -l < /tmp/slide_changes.txt)
echo ""
echo "========================================"
echo "Found $count potential talk boundaries"
echo "========================================"

if [ "$count" -gt 0 ]; then
    echo ""
    echo "To preview each cut point:"
    echo "  ffmpeg -ss TIMESTAMP -i \"$VIDEO\" -vframes 1 -q:v 2 preview.jpg && img2sixel preview.jpg"
fi
