#!/bin/bash
# Find talk boundaries in a conference video
# Looks for major scene changes combined with silence gaps
# Usage: ./find-talks.sh <video_file> [min_gap_seconds]
#
# min_gap_seconds: minimum silence duration to consider a boundary (default: 2)

VIDEO="${1:?Usage: $0 <video_file> [min_gap_seconds]}"
MIN_GAP="${2:-2}"

if [ ! -f "$VIDEO" ]; then
    echo "File not found: $VIDEO"
    exit 1
fi

echo "========================================"
echo "Analyzing: $(basename "$VIDEO")"
echo "Min silence gap: ${MIN_GAP}s"
echo "========================================"
echo ""

# Get video duration
duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$VIDEO" 2>/dev/null)
echo "Video duration: $(printf '%02d:%02d:%02d' $((${duration%.*}/3600)) $((${duration%.*}%3600/60)) $((${duration%.*}%60)))"
echo ""

# Detect silence periods (potential talk boundaries)
echo "Detecting silence gaps (this may take a while)..."
echo ""

tmpfile=$(mktemp /tmp/silence_XXXXXX.txt)
trap "rm -f $tmpfile" EXIT

# silencedetect finds periods of silence
# -50dB threshold, minimum duration of MIN_GAP seconds
ffmpeg -i "$VIDEO" -af "silencedetect=noise=-50dB:d=$MIN_GAP" -f null - 2>&1 | \
    grep -E "silence_(start|end)" > "$tmpfile"

echo "Potential talk boundaries (silence gaps):"
echo "----------------------------------------"

# Parse silence start/end pairs
grep "silence_start" "$tmpfile" | while read -r line; do
    start=$(echo "$line" | sed -n 's/.*silence_start: \([0-9.]*\).*/\1/p')
    if [ -n "$start" ]; then
        hours=$(echo "$start / 3600" | bc)
        mins=$(echo "($start % 3600) / 60" | bc)
        secs=$(echo "$start % 60" | bc)
        printf "  %02d:%02d:%05.2f\n" "$hours" "$mins" "$secs"
    fi
done

echo ""
echo "========================================"
echo ""
echo "To preview a cut point, run:"
echo "  ./preview.sh \"$VIDEO\" <timestamp>"
echo ""
echo "To extract a segment:"
echo "  ffmpeg -ss START -to END -i \"$VIDEO\" -c copy output.mp4"
