#!/usr/bin/env python3
"""
Match audio files to videos using transcription comparison.

Uses Whisper to transcribe both the video audio and external audio files,
then matches them by comparing the transcribed text.

Usage: uv run --with openai-whisper match-audio-transcription.py --dry-run

Requires: openai-whisper, torch
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
from difflib import SequenceMatcher
from pathlib import Path

# Configuration
TAGS_FILE = os.path.expanduser("~/videos/quadrant-tags.json")
AUDIO_DIR = os.path.expanduser("~/downloads/Second Room Recordings/Audio")
PROCESSED_VIDEO_DIR = os.path.expanduser("~/videos/take-2")
OUTPUT_DIR = os.path.expanduser("~/videos/synced")
TRANSCRIPTS_CACHE = os.path.expanduser("~/videos/transcripts.json")

# How much audio to transcribe for matching (seconds)
TRANSCRIBE_DURATION = 120


def is_second_room_video(path: str) -> bool:
    """Check if a video is from the Second Room."""
    return "Second Room" in path


def extract_audio(input_file: str, output_file: str, duration: int = None) -> bool:
    """Extract audio from video/audio file to WAV."""
    cmd = [
        "ffmpeg", "-y", "-hide_banner", "-loglevel", "error",
        "-i", input_file,
    ]
    if duration:
        cmd.extend(["-t", str(duration)])
    cmd.extend([
        "-ac", "1",
        "-ar", "16000",  # Whisper expects 16kHz
        "-f", "wav",
        output_file
    ])
    result = subprocess.run(cmd, capture_output=True)
    return result.returncode == 0


def transcribe_audio(audio_file: str, model) -> str:
    """Transcribe audio file using Whisper."""
    result = model.transcribe(audio_file, language="en", fp16=False)
    return result["text"].strip()


def text_similarity(text1: str, text2: str) -> float:
    """Calculate similarity between two texts (0-1)."""
    # Normalize texts
    t1 = text1.lower().split()
    t2 = text2.lower().split()

    # Use SequenceMatcher for fuzzy matching
    matcher = SequenceMatcher(None, t1, t2)
    return matcher.ratio()


def load_transcripts_cache() -> dict:
    """Load cached transcripts."""
    if Path(TRANSCRIPTS_CACHE).exists():
        with open(TRANSCRIPTS_CACHE) as f:
            return json.load(f)
    return {}


def save_transcripts_cache(cache: dict):
    """Save transcripts cache."""
    with open(TRANSCRIPTS_CACHE, 'w') as f:
        json.dump(cache, f, indent=2)


def find_audio_files(audio_dir: str) -> list[Path]:
    """Find all audio files in directory."""
    audio_path = Path(audio_dir)
    audio_files = []
    for ext in ['*.wav', '*.mp3', '*.m4a', '*.aac', '*.flac', '*.ogg', '*.WAV', '*.MP3']:
        audio_files.extend(audio_path.glob(f"**/{ext}"))
    return sorted(audio_files)


def get_audio_duration(file_path: str) -> float:
    """Get duration of audio/video file in seconds."""
    cmd = [
        "ffprobe", "-v", "error",
        "-show_entries", "format=duration",
        "-of", "csv=p=0",
        file_path
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    try:
        return float(result.stdout.strip())
    except:
        return 0


def find_time_offset_by_words(video_transcript: str, audio_transcript: str) -> float:
    """
    Estimate time offset by finding where in the audio the video text appears.
    Returns offset in seconds (positive = audio starts before video).
    """
    # This is a rough estimate - for precise sync we'd need word timestamps
    video_words = video_transcript.lower().split()[:50]  # First 50 words
    audio_words = audio_transcript.lower().split()

    if not video_words or not audio_words:
        return 0

    # Find best match position in audio
    best_pos = 0
    best_score = 0

    for i in range(len(audio_words) - len(video_words) + 1):
        chunk = audio_words[i:i + len(video_words)]
        score = SequenceMatcher(None, video_words, chunk).ratio()
        if score > best_score:
            best_score = score
            best_pos = i

    # Estimate time offset (rough: assume ~2 words per second)
    words_per_second = 2.5
    offset_seconds = best_pos / words_per_second

    return offset_seconds


def replace_audio(video_file: str, audio_file: str, offset_seconds: float, output_file: str) -> bool:
    """Replace video audio with synced external audio."""

    if offset_seconds >= 0:
        # Trim start of external audio
        audio_filter = f"atrim=start={offset_seconds},asetpts=PTS-STARTPTS"
    else:
        # Delay the external audio
        audio_filter = f"adelay={int(-offset_seconds * 1000)}|{int(-offset_seconds * 1000)}"

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
    parser = argparse.ArgumentParser(description="Match audio using transcription")
    parser.add_argument("--dry-run", action="store_true", help="Only show matches, don't process")
    parser.add_argument("--model", default="base", help="Whisper model (tiny/base/small/medium/large)")
    parser.add_argument("--audio-dir", default=AUDIO_DIR, help="Audio files directory")
    parser.add_argument("--output-dir", default=OUTPUT_DIR, help="Output directory")

    args = parser.parse_args()

    # Import whisper here so we fail fast if not installed
    try:
        import whisper
    except ImportError:
        print("Whisper not installed. Run:")
        print("  uv run --with openai-whisper --with torch <script>")
        sys.exit(1)

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
        print("No Second Room videos found")
        sys.exit(1)

    print(f"Found {len(second_room_videos)} Second Room video(s)")

    # Find audio files
    audio_files = find_audio_files(args.audio_dir)
    if not audio_files:
        print(f"No audio files found in {args.audio_dir}")
        sys.exit(1)

    print(f"Found {len(audio_files)} audio file(s)")

    # Load Whisper model
    print(f"\nLoading Whisper model '{args.model}'...")
    model = whisper.load_model(args.model)
    print("Model loaded!\n")

    # Load transcript cache
    cache = load_transcripts_cache()

    # Create output directory
    Path(args.output_dir).mkdir(parents=True, exist_ok=True)

    # Transcribe all audio files first
    print("=" * 60)
    print("TRANSCRIBING AUDIO FILES")
    print("=" * 60)

    audio_transcripts = {}
    with tempfile.TemporaryDirectory() as tmpdir:
        for i, audio_file in enumerate(audio_files, 1):
            cache_key = f"audio:{audio_file}"

            if cache_key in cache:
                print(f"[{i}/{len(audio_files)}] {audio_file.name} (cached)")
                audio_transcripts[str(audio_file)] = cache[cache_key]
                continue

            print(f"[{i}/{len(audio_files)}] Transcribing {audio_file.name}...")

            wav_file = f"{tmpdir}/audio.wav"
            if not extract_audio(str(audio_file), wav_file, TRANSCRIBE_DURATION):
                print("  Failed to extract audio")
                continue

            transcript = transcribe_audio(wav_file, model)
            audio_transcripts[str(audio_file)] = transcript
            cache[cache_key] = transcript

            # Show preview
            preview = transcript[:100] + "..." if len(transcript) > 100 else transcript
            print(f"  \"{preview}\"")

        save_transcripts_cache(cache)

    # Now match videos
    print("\n" + "=" * 60)
    print("MATCHING VIDEOS TO AUDIO")
    print("=" * 60 + "\n")

    results = []

    with tempfile.TemporaryDirectory() as tmpdir:
        for i, (name, data) in enumerate(second_room_videos.items(), 1):
            processed_name = Path(name).stem + ".mp4"
            video_path = os.path.join(PROCESSED_VIDEO_DIR, processed_name)

            print(f"[{i}/{len(second_room_videos)}] {name}")

            if not Path(video_path).exists():
                print(f"  Video not found: {video_path}")
                results.append((name, None, 0, 0))
                continue

            # Get video transcript (from cache or transcribe)
            cache_key = f"video:{video_path}"
            if cache_key in cache:
                video_transcript = cache[cache_key]
                print("  Using cached transcript")
            else:
                print("  Transcribing video audio...")
                wav_file = f"{tmpdir}/video.wav"
                if not extract_audio(video_path, wav_file, TRANSCRIBE_DURATION):
                    print("  Failed to extract audio")
                    results.append((name, None, 0, 0))
                    continue

                video_transcript = transcribe_audio(wav_file, model)
                cache[cache_key] = video_transcript
                save_transcripts_cache(cache)

            # Find best matching audio
            best_match = None
            best_score = 0
            best_offset = 0

            for audio_file, audio_transcript in audio_transcripts.items():
                score = text_similarity(video_transcript, audio_transcript)

                if score > best_score:
                    best_score = score
                    best_match = audio_file
                    # Estimate offset
                    best_offset = find_time_offset_by_words(video_transcript, audio_transcript)

            if best_match:
                match_name = Path(best_match).name
                print(f"  Match: {match_name}")
                print(f"  Score: {best_score:.2%}, Offset: ~{best_offset:.1f}s")
                results.append((name, match_name, best_score, best_offset))

                if not args.dry_run and best_score > 0.3:
                    output_file = os.path.join(args.output_dir, processed_name.replace('.mp4', '_synced.mp4'))
                    print(f"  Syncing audio...")
                    if replace_audio(video_path, best_match, best_offset, output_file):
                        print(f"  Output: {output_file}")
                    else:
                        print("  FAILED to sync")
            else:
                print("  No match found!")
                results.append((name, None, 0, 0))

            print()

    # Summary
    print("=" * 60)
    print("SUMMARY")
    print("=" * 60)
    print(f"{'Video':<30} {'Audio Match':<25} {'Score':<8}")
    print("-" * 60)
    for name, match, score, offset in results:
        name_short = name[:28] if len(name) > 28 else name
        match_short = (match[:22] + "...") if match and len(match) > 25 else (match or "NO MATCH")
        print(f"{name_short:<30} {match_short:<25} {score:.0%}")


if __name__ == "__main__":
    main()
