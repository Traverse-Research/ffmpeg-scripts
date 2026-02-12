#!/bin/bash
# Process all tagged videos using FFmpeg with background compositing
# Usage: ./process-videos.sh [background_image] [output_dir]
#
# Reads: ~/videos/quadrant-tags.json
# Requires: ffmpeg, jq

# Don't use set -e as it causes issues with while loops
# set -e

BG_IMAGE="${1:-$HOME/gpc-bg.png}"
OUTPUT_DIR="${2:-$HOME/videos/processed}"
TAGS_FILE="$HOME/videos/quadrant-tags.json"

# Check dependencies
if ! command -v ffmpeg &> /dev/null; then
    echo "ffmpeg not found"
    exit 1
fi

if ! command -v jq &> /dev/null; then
    echo "jq not found. Install with: apt install jq"
    exit 1
fi

# Check tags file exists
if [ ! -f "$TAGS_FILE" ]; then
    echo "Tags file not found: $TAGS_FILE"
    echo "Run tag-videos.sh first to create it"
    exit 1
fi

# Check background image exists
if [ ! -f "$BG_IMAGE" ]; then
    echo "Background image not found: $BG_IMAGE"
    echo "Download it first or provide path as first argument"
    exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Function to get crop parameters for a quadrant
# Video is 3840x2160 (4K), divided into 4 quadrants of 1920x1080 each
# We apply a 4px offset to trim borders
get_crop() {
    local quadrant="$1"
    case "$quadrant" in
        "top-left")     echo "1912:1072:4:4" ;;
        "top-right")    echo "1912:1072:1924:4" ;;
        "bottom-left")  echo "1912:1072:4:1084" ;;
        "bottom-right") echo "1912:1072:1924:1084" ;;
        *)
            echo "Invalid quadrant: $quadrant" >&2
            return 1
            ;;
    esac
}

# Build FFmpeg filter complex
# Input 0: video, Input 1: background image
# Output: composited video with slides large and presenter small in corner
build_filter() {
    local presenter_crop="$1"
    local slides_crop="$2"

    # Filter explanation:
    # 1. Scale background to 2560x1440
    # 2. Crop slides quadrant and scale to 1920x1080 (big, centered)
    # 3. Crop presenter quadrant and scale to 320px height (small, corner)
    # 4. Overlay slides centered on background
    # 5. Overlay presenter in bottom-right corner with 40px margin
    echo "[1:v]scale=2560:1440[bg]; \
[0:v]crop=${slides_crop}[slides_cropped]; \
[slides_cropped]scale=1920:1080[slides]; \
[0:v]crop=${presenter_crop}[presenter_raw]; \
[presenter_raw]scale=-1:320[presenter]; \
[slides]scale=1920:1080[slides_s]; \
[bg][slides_s]overlay=(W-w)/2:(H-h)/2[base]; \
[base][presenter]overlay=x=W-w-40:y=H-h-40[outv]"
}

# Count total videos
total=$(jq 'length' "$TAGS_FILE")
current=0

echo "========================================"
echo "Processing $total video(s)"
echo "Background: $BG_IMAGE"
echo "Output dir: $OUTPUT_DIR"
echo "========================================"
echo ""

# Process each video - write entries to temp file to avoid subshell issues
tmpentries=$(mktemp)
jq -r 'to_entries[] | @base64' "$TAGS_FILE" 2>/dev/null | grep -v '^$' > "$tmpentries"

while IFS= read -r entry; do
    # Decode the entry - skip if decoding fails
    decoded=$(echo "$entry" | base64 -d 2>/dev/null)
    if [ -z "$decoded" ]; then
        continue
    fi
    filename=$(echo "$decoded" | jq -r '.key' 2>/dev/null) || continue
    [ -z "$filename" ] && continue
    # Sanity check - filename should end in .mov
    [[ "$filename" != *.mov ]] && continue
    presenter=$(echo "$decoded" | jq -r '.value.presenter' 2>/dev/null)
    slides=$(echo "$decoded" | jq -r '.value.slides' 2>/dev/null)
    input_path=$(echo "$decoded" | jq -r '.value.path' 2>/dev/null)

    current=$((current + 1))

    # Output filename (change extension to .mp4)
    output_file="$OUTPUT_DIR/${filename%.*}.mp4"

    # Skip if already processed
    if [ -f "$output_file" ]; then
        echo "[$current/$total] Skipping $filename (already exists)"
        continue
    fi

    echo "[$current/$total] Processing: $filename"
    echo "  Input: $input_path"
    echo "  Presenter: $presenter"
    echo "  Slides: $slides"
    echo "  Output: $output_file"

    # Get crop parameters
    presenter_crop=$(get_crop "$presenter") || { echo "  Skipping - invalid presenter quadrant"; continue; }
    slides_crop=$(get_crop "$slides") || { echo "  Skipping - invalid slides quadrant"; continue; }

    # Build filter
    filter=$(build_filter "$presenter_crop" "$slides_crop")

    # Run FFmpeg
    echo "  Running FFmpeg..."
    if ffmpeg -y \
        -i "$input_path" \
        -i "$BG_IMAGE" \
        -filter_complex "$filter" \
        -map "[outv]" \
        -map "0:a?" \
        -c:v libx264 \
        -crf 18 \
        -preset veryfast \
        -threads 0 \
        -c:a copy \
        "$output_file" \
        2>&1 | tail -5; then
        echo "  Done!"
    else
        echo "  FAILED!"
    fi
    echo ""
done < "$tmpentries"

rm -f "$tmpentries"

echo "========================================"
echo "Processing complete!"
echo "Output directory: $OUTPUT_DIR"
echo "========================================"

# Show summary
echo ""
echo "Processed files:"
ls -lh "$OUTPUT_DIR"/*.mp4 2>/dev/null || echo "  No files processed"
