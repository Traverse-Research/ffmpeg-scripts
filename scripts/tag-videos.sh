#!/bin/bash
# Tag video quadrants interactively using thumbnails displayed via img2sixel
# Usage: ./tag-videos.sh <directory>
#
# Requires: ffmpeg, img2sixel (from libsixel-bin)
# Output: ~/videos/quadrant-tags.json

set -e

VIDEO_DIR="${1:?Usage: $0 <directory>}"
OUTPUT_FILE="$HOME/videos/quadrant-tags.json"

# Create output directory
mkdir -p "$HOME/videos"

# Initialize JSON file if it doesn't exist
if [ ! -f "$OUTPUT_FILE" ]; then
    echo "{}" > "$OUTPUT_FILE"
fi

# Find all .mov files
mapfile -t videos < <(find "$VIDEO_DIR" -type f -name "*.mov" | sort)

if [ ${#videos[@]} -eq 0 ]; then
    echo "No .mov files found in $VIDEO_DIR"
    exit 1
fi

echo "Found ${#videos[@]} video(s) to tag"
echo ""

# Process each video
for video in "${videos[@]}"; do
    filename=$(basename "$video")

    # Check if already tagged
    if jq -e ".\"$filename\"" "$OUTPUT_FILE" > /dev/null 2>&1; then
        echo "Skipping $filename (already tagged)"
        continue
    fi

    echo "========================================"
    echo "Processing: $filename"
    echo "========================================"

    # Create temp file for thumbnail
    tmpfile=$(mktemp /tmp/thumb_XXXXXX.jpg)
    trap "rm -f $tmpfile" EXIT

    # Get video duration and extract frame from middle
    duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$video" 2>/dev/null | cut -d'.' -f1)
    if [ -z "$duration" ] || [ "$duration" -lt 2 ]; then
        seek=0
    else
        seek=$((duration / 2))
    fi
    ffmpeg -y -ss "$seek" -i "$video" -vframes 1 -q:v 2 "$tmpfile" 2>/dev/null

    # Display the thumbnail
    echo ""
    img2sixel -w 800 "$tmpfile"
    echo ""

    # Show quadrant layout
    echo "Quadrant layout:"
    echo "  +-------------+-------------+"
    echo "  |     TL      |     TR      |"
    echo "  | (top-left)  | (top-right) |"
    echo "  +-------------+-------------+"
    echo "  |     BL      |     BR      |"
    echo "  | (bot-left)  | (bot-right) |"
    echo "  +-------------+-------------+"
    echo ""

    # Ask for presenter quadrant
    while true; do
        read -p "Presenter quadrant (TL/TR/BL/BR) or 's' to skip: " presenter
        presenter=$(echo "$presenter" | tr '[:lower:]' '[:upper:]')
        case "$presenter" in
            TL) presenter="top-left"; break;;
            TR) presenter="top-right"; break;;
            BL) presenter="bottom-left"; break;;
            BR) presenter="bottom-right"; break;;
            S) presenter=""; break;;
            *) echo "Invalid choice. Use TL, TR, BL, BR, or s to skip.";;
        esac
    done

    if [ -z "$presenter" ]; then
        echo "Skipping $filename"
        rm -f "$tmpfile"
        continue
    fi

    # Ask for slides quadrant
    while true; do
        read -p "Slides quadrant (TL/TR/BL/BR): " slides
        slides=$(echo "$slides" | tr '[:lower:]' '[:upper:]')
        case "$slides" in
            TL) slides="top-left"; break;;
            TR) slides="top-right"; break;;
            BL) slides="bottom-left"; break;;
            BR) slides="bottom-right"; break;;
            *) echo "Invalid choice. Use TL, TR, BL, or BR.";;
        esac
    done

    # Warn if same quadrant selected
    if [ "$presenter" = "$slides" ]; then
        echo "Warning: Presenter and slides are the same quadrant!"
        read -p "Continue anyway? (y/n): " confirm
        if [ "$confirm" != "y" ]; then
            rm -f "$tmpfile"
            continue
        fi
    fi

    # Save to JSON
    tmp_json=$(mktemp)
    jq --arg file "$filename" \
       --arg pres "$presenter" \
       --arg slides "$slides" \
       --arg path "$video" \
       '.[$file] = {"presenter": $pres, "slides": $slides, "path": $path}' \
       "$OUTPUT_FILE" > "$tmp_json"
    mv "$tmp_json" "$OUTPUT_FILE"

    echo "Saved: $filename -> presenter=$presenter, slides=$slides"
    echo ""

    rm -f "$tmpfile"
done

echo "========================================"
echo "Tagging complete!"
echo "Results saved to: $OUTPUT_FILE"
echo "========================================"

# Show summary
echo ""
echo "Tagged videos:"
jq -r 'to_entries[] | "  \(.key): presenter=\(.value.presenter), slides=\(.value.slides)"' "$OUTPUT_FILE"
