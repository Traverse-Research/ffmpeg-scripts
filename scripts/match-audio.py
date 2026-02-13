#!/usr/bin/env python3
"""
Match and sync external audio files to videos.

This script:
1. Extracts audio from video files
2. Compares with external audio files using cross-correlation
3. Finds the best match and time offset
4. Replaces the video audio with the synced high-quality audio

Usage: ./match-audio.py <video_file> <audio_dir> [--output <output_file>]

Example:
  ./match-audio.py "video.mp4" "Second Room Recordings/Audio/" --output "video_synced.mp4"
"""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

# Duration of audio to analyze for matching (seconds)
ANALYSIS_DURATION = 60
# Sample rate for analysis (lower = faster but less accurate)
ANALYSIS_SAMPLE_RATE = 8000


def extract_audio(input_file: str, output_file: str, duration: int = None, sample_rate: int = 44100) -> bool:
    """Extract audio from video/audio file to WAV."""
    cmd = [
        "ffmpeg", "-y",
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
        # Normalize
        samples = samples / 32768.0
    return samples


def cross_correlate(signal1: np.ndarray, signal2: np.ndarray) -> tuple[float, float]:
    """
    Find the best alignment between two signals using cross-correlation.
    Returns (offset_samples, correlation_score).

    Positive offset means signal2 starts after signal1.
    """
    # Use FFT-based correlation for speed
    from scipy import signal as sig

    # Normalize signals
    s1 = (signal1 - np.mean(signal1)) / (np.std(signal1) + 1e-10)
    s2 = (signal2 - np.mean(signal2)) / (np.std(signal2) + 1e-10)

    # Cross-correlate
    correlation = sig.correlate(s1, s2, mode='full')

    # Find peak
    peak_idx = np.argmax(np.abs(correlation))
    peak_value = np.abs(correlation[peak_idx])

    # Calculate offset (positive = s2 starts later)
    offset = peak_idx - len(s2) + 1

    # Normalize correlation score
    score = peak_value / len(s1)

    return offset, score


def find_best_match(video_file: str, audio_dir: str) -> tuple[str, float, float]:
    """
    Find the best matching audio file and time offset.
    Returns (audio_file, offset_seconds, score).
    """
    audio_path = Path(audio_dir)

    # Find all audio files
    audio_files = []
    for ext in ['*.wav', '*.mp3', '*.m4a', '*.aac', '*.flac', '*.ogg']:
        audio_files.extend(audio_path.glob(f"**/{ext}"))

    if not audio_files:
        print(f"No audio files found in {audio_dir}")
        return None, 0, 0

    print(f"Found {len(audio_files)} audio file(s) to compare")
    print()

    with tempfile.TemporaryDirectory() as tmpdir:
        # Extract audio from video
        video_audio = f"{tmpdir}/video_audio.wav"
        print("Extracting audio from video...")
        if not extract_audio(video_file, video_audio, ANALYSIS_DURATION, ANALYSIS_SAMPLE_RATE):
            print("Failed to extract audio from video")
            return None, 0, 0

        video_samples = load_audio_samples(video_audio)
        print(f"Video audio: {len(video_samples)} samples ({len(video_samples)/ANALYSIS_SAMPLE_RATE:.1f}s)")
        print()

        best_match = None
        best_score = 0
        best_offset = 0

        for i, audio_file in enumerate(audio_files, 1):
            print(f"[{i}/{len(audio_files)}] Comparing: {audio_file.name}...", end=" ", flush=True)

            # Extract/convert audio file
            ext_audio = f"{tmpdir}/ext_audio_{i}.wav"
            if not extract_audio(str(audio_file), ext_audio, ANALYSIS_DURATION * 2, ANALYSIS_SAMPLE_RATE):
                print("SKIP (extraction failed)")
                continue

            ext_samples = load_audio_samples(ext_audio)

            # Cross-correlate
            offset_samples, score = cross_correlate(video_samples, ext_samples)
            offset_seconds = offset_samples / ANALYSIS_SAMPLE_RATE

            print(f"score={score:.4f}, offset={offset_seconds:+.2f}s")

            if score > best_score:
                best_score = score
                best_match = str(audio_file)
                best_offset = offset_seconds

    return best_match, best_offset, best_score


def replace_audio(video_file: str, audio_file: str, offset_seconds: float, output_file: str) -> bool:
    """Replace video audio with synced external audio."""

    # If offset is positive, external audio starts later, so we delay it
    # If offset is negative, external audio starts earlier, so we trim it

    if offset_seconds >= 0:
        # Delay the external audio
        audio_filter = f"adelay={int(offset_seconds * 1000)}|{int(offset_seconds * 1000)}"
    else:
        # Trim the start of external audio
        audio_filter = f"atrim=start={-offset_seconds}"

    cmd = [
        "ffmpeg", "-y",
        "-i", video_file,
        "-i", audio_file,
        "-c:v", "copy",
        "-af", audio_filter,
        "-map", "0:v:0",
        "-map", "1:a:0",
        "-shortest",
        output_file
    ]

    print(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True)

    if result.returncode != 0:
        print(f"FFmpeg error: {result.stderr[-500:]}")
        return False

    return True


def main():
    parser = argparse.ArgumentParser(description="Match and sync external audio to video")
    parser.add_argument("video_file", help="Video file to process")
    parser.add_argument("audio_dir", help="Directory containing audio files to match")
    parser.add_argument("--output", "-o", help="Output video file (default: video_synced.mp4)")
    parser.add_argument("--threshold", "-t", type=float, default=0.1,
                        help="Minimum correlation score to accept match (default: 0.1)")

    args = parser.parse_args()

    if not Path(args.video_file).exists():
        print(f"Video file not found: {args.video_file}")
        sys.exit(1)

    if not Path(args.audio_dir).exists():
        print(f"Audio directory not found: {args.audio_dir}")
        sys.exit(1)

    output_file = args.output or Path(args.video_file).stem + "_synced.mp4"

    print("=" * 50)
    print(f"Video: {args.video_file}")
    print(f"Audio dir: {args.audio_dir}")
    print(f"Output: {output_file}")
    print("=" * 50)
    print()

    # Find best match
    best_match, offset, score = find_best_match(args.video_file, args.audio_dir)

    if not best_match:
        print("No matching audio found!")
        sys.exit(1)

    print()
    print("=" * 50)
    print(f"Best match: {best_match}")
    print(f"Score: {score:.4f}")
    print(f"Offset: {offset:+.3f}s")
    print("=" * 50)

    if score < args.threshold:
        print(f"\nWarning: Score {score:.4f} is below threshold {args.threshold}")
        print("Match may not be reliable!")
        response = input("Continue anyway? [y/N] ")
        if response.lower() != 'y':
            sys.exit(1)

    print()
    print("Replacing audio...")

    if replace_audio(args.video_file, best_match, offset, output_file):
        print(f"\nSuccess! Output: {output_file}")
    else:
        print("\nFailed to replace audio")
        sys.exit(1)


if __name__ == "__main__":
    main()
