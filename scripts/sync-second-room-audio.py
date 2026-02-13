#!/usr/bin/env python3
"""
Sync high-quality audio for all Second Room videos.

Reads the quadrant-tags.json file, identifies Second Room videos,
matches them with audio files, and creates synced versions.

Usage: ./sync-second-room-audio.py [--dry-run]

Requires: numpy, scipy
Install: pip install numpy scipy
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from scipy import signal as sig

# Configuration
TAGS_FILE = os.path.expanduser("~/videos/quadrant-tags.json")
AUDIO_DIR = os.path.expanduser("~/downloads/Second Room Recordings/Audio")
PROCESSED_VIDEO_DIR = os.path.expanduser("~/videos/take-2")
OUTPUT_DIR = os.path.expanduser("~/videos/synced")

# Analysis settings
ANALYSIS_DURATION = 120  # seconds of audio to analyze
ANALYSIS_SAMPLE_RATE = 8000  # Hz (lower = faster)
MIN_SCORE_THRESHOLD = 0.05  # minimum correlation score to accept


def is_second_room_video(path: str) -> bool:
    """Check if a video is from the Second Room."""
    return "Second Room" in path


def extract_audio(input_file: str, output_file: str, duration: int = None, sample_rate: int = 44100) -> bool:
    """Extract audio from video/audio file to WAV."""
    cmd = [
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-i", input_file,
    ]
    if duration:
        cmd.extend(["-t", str(duration)])
    cmd.extend([
        "-ac", "1",  # mono
        "-ar", str(sample_rate),
        "-f", "wav",
        output_file
    ])

    result = subprocess.run(cmd, capture_output=True)
    return result.returncode == 0


def load_audio_samples(wav_file: str) -> np.ndarray:
    """Load WAV file as numpy array."""
    import wave
    with wave.open(wav_file, 'rb') as wf:
        n_frames = wf.getnframes()
        audio_data = wf.readframes(n_frames)
        samples = np.frombuffer(audio_data, dtype=np.int16).astype(np.float32)
        samples = samples / 32768.0  # Normalize
    return samples


def cross_correlate(signal1: np.ndarray, signal2: np.ndarray) -> tuple[float, float]:
    """Find best alignment using cross-correlation."""
    s1 = (signal1 - np.mean(signal1)) / (np.std(signal1) + 1e-10)
    s2 = (signal2 - np.mean(signal2)) / (np.std(signal2) + 1e-10)

    correlation = sig.correlate(s1, s2, mode='full')
    peak_idx = np.argmax(np.abs(correlation))
    peak_value = np.abs(correlation[peak_idx])
    offset = peak_idx - len(s2) + 1
    score = peak_value / len(s1)

    return offset, score


def find_audio_files(audio_dir: str) -> list[Path]:
    """Find all audio files in directory."""
    audio_path = Path(audio_dir)
    audio_files = []
    for ext in ['*.wav', '*.mp3', '*.m4a', '*.aac', '*.flac', '*.ogg', '*.WAV', '*.MP3']:
        audio_files.extend(audio_path.glob(f"**/{ext}"))
    return sorted(audio_files)


def find_best_match(video_file: str, audio_files: list[Path], tmpdir: str, video_samples: np.ndarray = None) -> tuple[str, float, float, np.ndarray]:
    """Find best matching audio file."""

    # Extract video audio if not provided
    if video_samples is None:
        video_audio = f"{tmpdir}/video_audio.wav"
        if not extract_audio(video_file, video_audio, ANALYSIS_DURATION, ANALYSIS_SAMPLE_RATE):
            return None, 0, 0, None
        video_samples = load_audio_samples(video_audio)

    best_match = None
    best_score = 0
    best_offset = 0

    for audio_file in audio_files:
        ext_audio = f"{tmpdir}/ext_audio.wav"
        if not extract_audio(str(audio_file), ext_audio, ANALYSIS_DURATION * 2, ANALYSIS_SAMPLE_RATE):
            continue

        ext_samples = load_audio_samples(ext_audio)
        offset_samples, score = cross_correlate(video_samples, ext_samples)
        offset_seconds = offset_samples / ANALYSIS_SAMPLE_RATE

        if score > best_score:
            best_score = score
            best_match = str(audio_file)
            best_offset = offset_seconds

    return best_match, best_offset, best_score, video_samples


def replace_audio(video_file: str, audio_file: str, offset_seconds: float, output_file: str) -> bool:
    """Replace video audio with synced external audio."""

    if offset_seconds >= 0:
        audio_filter = f"adelay={int(offset_seconds * 1000)}|{int(offset_seconds * 1000)}"
    else:
        audio_filter = f"atrim=start={-offset_seconds}"

    cmd = [
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-i", video_file,
        "-i", audio_file,
        "-c:v", "copy",
        "-af", audio_filter,
        "-map", "0:v:0",
        "-map", "1:a:0",
        "-shortest",
        output_file
    ]

    result = subprocess.run(cmd, capture_output=True, text=True)
    return result.returncode == 0


def main():
    parser = argparse.ArgumentParser(description="Sync audio for Second Room videos")
    parser.add_argument("--dry-run", action="store_true", help="Only show matches, don't process")
    parser.add_argument("--audio-dir", default=AUDIO_DIR, help="Audio files directory")
    parser.add_argument("--output-dir", default=OUTPUT_DIR, help="Output directory")

    args = parser.parse_args()

    # Load tags
    if not Path(TAGS_FILE).exists():
        print(f"Tags file not found: {TAGS_FILE}")
        sys.exit(1)

    with open(TAGS_FILE) as f:
        tags = json.load(f)

    # Filter to Second Room videos
    second_room_videos = {
        name: data for name, data in tags.items()
        if is_second_room_video(data.get("path", ""))
    }

    if not second_room_videos:
        print("No Second Room videos found in tags file")
        sys.exit(1)

    print(f"Found {len(second_room_videos)} Second Room video(s)")

    # Find audio files
    audio_files = find_audio_files(args.audio_dir)
    if not audio_files:
        print(f"No audio files found in {args.audio_dir}")
        sys.exit(1)

    print(f"Found {len(audio_files)} audio file(s)")
    print()

    # Create output directory
    Path(args.output_dir).mkdir(parents=True, exist_ok=True)

    # Process each video
    results = []

    with tempfile.TemporaryDirectory() as tmpdir:
        for i, (name, data) in enumerate(second_room_videos.items(), 1):
            # Use processed video from take-2 directory (same filename logic as process-videos.py)
            # Original: "2025-11-18 10-29-29.mov" -> Processed: "2025-11-18 10-29-29.mp4"
            processed_name = Path(name).stem + ".mp4"
            video_path = os.path.join(PROCESSED_VIDEO_DIR, processed_name)

            print(f"[{i}/{len(second_room_videos)}] {name}")

            if not Path(video_path).exists():
                print(f"  Processed video not found: {video_path}")
                continue

            print(f"  Analyzing audio...")
            best_match, offset, score, _ = find_best_match(video_path, audio_files, tmpdir)

            if not best_match:
                print(f"  No match found!")
                results.append((name, None, 0, 0))
                continue

            match_name = Path(best_match).name
            print(f"  Match: {match_name}")
            print(f"  Score: {score:.4f}, Offset: {offset:+.2f}s")

            if score < MIN_SCORE_THRESHOLD:
                print(f"  WARNING: Score below threshold ({MIN_SCORE_THRESHOLD})")

            results.append((name, match_name, score, offset))

            if not args.dry_run and score >= MIN_SCORE_THRESHOLD:
                output_file = os.path.join(args.output_dir, name.replace('.mov', '_synced.mp4'))
                print(f"  Syncing audio...")
                if replace_audio(video_path, best_match, offset, output_file):
                    print(f"  Output: {output_file}")
                else:
                    print(f"  FAILED to sync audio")

            print()

    # Summary
    print("=" * 60)
    print("SUMMARY")
    print("=" * 60)
    print(f"{'Video':<30} {'Audio Match':<20} {'Score':<8} {'Offset'}")
    print("-" * 60)
    for name, match, score, offset in results:
        match_short = match[:17] + "..." if match and len(match) > 20 else (match or "NO MATCH")
        print(f"{name[:28]:<30} {match_short:<20} {score:.4f}   {offset:+.2f}s")


if __name__ == "__main__":
    main()
