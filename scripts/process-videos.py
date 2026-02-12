#!/usr/bin/env python3
"""
Process all tagged videos using FFmpeg with background compositing.

Usage: ./process-videos.py [background_image] [output_dir]

Reads: ~/videos/quadrant-tags.json
Requires: ffmpeg
"""

import json
import os
import subprocess
import sys
from pathlib import Path


def get_crop(quadrant: str) -> str:
    """Get FFmpeg crop parameters for a quadrant."""
    crops = {
        "top-left": "1912:1072:4:4",
        "top-right": "1912:1072:1924:4",
        "bottom-left": "1912:1072:4:1084",
        "bottom-right": "1912:1072:1924:1084",
    }
    if quadrant not in crops:
        raise ValueError(f"Invalid quadrant: {quadrant}")
    return crops[quadrant]


def build_filter(presenter_crop: str, slides_crop: str) -> str:
    """Build FFmpeg filter complex.

    Output: composited video with slides large and presenter small in corner.
    """
    return (
        f"[1:v]scale=2560:1440[bg]; "
        f"[0:v]crop={slides_crop}[slides_cropped]; "
        f"[slides_cropped]scale=1920:1080[slides]; "
        f"[0:v]crop={presenter_crop}[presenter_raw]; "
        f"[presenter_raw]scale=-1:320[presenter]; "
        f"[slides]scale=1920:1080[slides_s]; "
        f"[bg][slides_s]overlay=(W-w)/2:(H-h)/2[base]; "
        f"[base][presenter]overlay=x=W-w-40:y=H-h-40[outv]"
    )


def process_video(input_path: str, output_path: str, bg_image: str,
                  presenter: str, slides: str) -> bool:
    """Process a single video with FFmpeg."""
    try:
        presenter_crop = get_crop(presenter)
        slides_crop = get_crop(slides)
    except ValueError as e:
        print(f"  Error: {e}")
        return False

    filter_complex = build_filter(presenter_crop, slides_crop)

    cmd = [
        "ffmpeg", "-y",
        "-i", input_path,
        "-i", bg_image,
        "-filter_complex", filter_complex,
        "-map", "[outv]",
        "-map", "0:a?",
        "-c:v", "libx264",
        "-crf", "18",
        "-preset", "veryfast",
        "-threads", "0",
        "-c:a", "copy",
        output_path
    ]

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True
        )
        if result.returncode != 0:
            print(f"  FFmpeg error: {result.stderr[-500:]}")
            return False
        return True
    except Exception as e:
        print(f"  Error running FFmpeg: {e}")
        return False


def main():
    # Parse arguments
    bg_image = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/gpc-bg.png")
    output_dir = sys.argv[2] if len(sys.argv) > 2 else os.path.expanduser("~/videos/processed")
    tags_file = os.path.expanduser("~/videos/quadrant-tags.json")

    # Check dependencies
    if not Path(bg_image).exists():
        print(f"Background image not found: {bg_image}")
        sys.exit(1)

    if not Path(tags_file).exists():
        print(f"Tags file not found: {tags_file}")
        print("Run tag-videos.sh first to create it")
        sys.exit(1)

    # Create output directory
    Path(output_dir).mkdir(parents=True, exist_ok=True)

    # Load tags
    with open(tags_file) as f:
        tags = json.load(f)

    total = len(tags)
    print("=" * 40)
    print(f"Processing {total} video(s)")
    print(f"Background: {bg_image}")
    print(f"Output dir: {output_dir}")
    print("=" * 40)
    print()

    # Process each video
    for i, (filename, data) in enumerate(tags.items(), 1):
        presenter = data["presenter"]
        slides = data["slides"]
        input_path = data["path"]

        # Output filename (change extension to .mp4)
        output_name = Path(filename).stem + ".mp4"
        output_path = os.path.join(output_dir, output_name)

        # Skip if already processed
        if Path(output_path).exists():
            print(f"[{i}/{total}] Skipping {filename} (already exists)")
            continue

        print(f"[{i}/{total}] Processing: {filename}")
        print(f"  Input: {input_path}")
        print(f"  Presenter: {presenter}")
        print(f"  Slides: {slides}")
        print(f"  Output: {output_path}")
        print("  Running FFmpeg...")

        if process_video(input_path, output_path, bg_image, presenter, slides):
            print("  Done!")
        else:
            print("  FAILED!")
        print()

    print("=" * 40)
    print("Processing complete!")
    print(f"Output directory: {output_dir}")
    print("=" * 40)

    # Show summary
    print()
    print("Processed files:")
    for f in Path(output_dir).glob("*.mp4"):
        size_mb = f.stat().st_size / (1024 * 1024)
        print(f"  {f.name}: {size_mb:.1f} MB")


if __name__ == "__main__":
    main()
