#!/usr/bin/env python3
"""
Upload a directory to WebDAV server.

Usage: ./upload-webdav.py <local_dir> <remote_dir> --url <webdav_url> --user <username> --pass <password>

Example:
  ./upload-webdav.py ~/videos/processed /processed --url https://nextcloud.example.com/remote.php/webdav --user admin --pass secret
"""

import argparse
import sys
from pathlib import Path

import requests
from requests.auth import HTTPBasicAuth


def create_folder(webdav_url: str, remote_path: str, username: str, password: str) -> bool:
    """Create a folder on WebDAV server."""
    webdav_url = webdav_url.rstrip('/')
    remote_path = remote_path.strip('/')
    url = f"{webdav_url}/{remote_path}"

    try:
        response = requests.request(
            "MKCOL",
            url,
            auth=HTTPBasicAuth(username, password),
        )
        # 201 = created, 405 = already exists
        return response.status_code in (201, 405)
    except Exception as e:
        print(f"Error creating folder: {e}")
        return False


def upload_file(local_path: Path, remote_path: str, webdav_url: str, username: str, password: str) -> bool:
    """Upload a single file to WebDAV server."""
    webdav_url = webdav_url.rstrip('/')
    remote_path = remote_path.lstrip('/')
    url = f"{webdav_url}/{remote_path}"

    file_size = local_path.stat().st_size
    print(f"  Uploading: {local_path.name} ({file_size / (1024*1024):.1f} MB)")

    try:
        with open(local_path, 'rb') as f:
            response = requests.put(
                url,
                data=f,
                auth=HTTPBasicAuth(username, password),
                headers={'Content-Type': 'application/octet-stream'},
            )

        if response.status_code in (200, 201, 204):
            print(f"    Done!")
            return True
        else:
            print(f"    Failed: {response.status_code} {response.reason}")
            return False

    except Exception as e:
        print(f"    Error: {e}")
        return False


def upload_directory(local_dir: str, remote_dir: str, webdav_url: str, username: str, password: str) -> tuple[int, int]:
    """Upload all files in a directory to WebDAV server."""
    local_path = Path(local_dir)

    if not local_path.exists():
        print(f"Error: Directory not found: {local_dir}")
        return 0, 0

    if not local_path.is_dir():
        print(f"Error: Not a directory: {local_dir}")
        return 0, 0

    # Get all files
    files = list(local_path.glob("*"))
    files = [f for f in files if f.is_file()]

    if not files:
        print("No files found in directory")
        return 0, 0

    print(f"Found {len(files)} file(s) to upload")
    print(f"Destination: {webdav_url}/{remote_dir.strip('/')}/")
    print()

    # Create remote directory
    remote_dir = remote_dir.strip('/')
    if remote_dir:
        print(f"Creating remote folder: {remote_dir}")
        create_folder(webdav_url, remote_dir, username, password)
        print()

    # Upload each file
    success = 0
    failed = 0

    for i, file in enumerate(files, 1):
        print(f"[{i}/{len(files)}]", end="")
        remote_path = f"{remote_dir}/{file.name}" if remote_dir else file.name

        if upload_file(file, remote_path, webdav_url, username, password):
            success += 1
        else:
            failed += 1

    return success, failed


WEBDAV_URL = "https://nx73934.your-storageshare.de/remote.php/webdav/"
WEBDAV_USER = "jasper"
WEBDAV_PASS = "ypp=UL2!mwBM7!Q"


def main():
    parser = argparse.ArgumentParser(description="Upload a directory to WebDAV server")
    parser.add_argument("local_dir", help="Local directory to upload")
    parser.add_argument("remote_dir", help="Remote directory (e.g., /processed)")

    args = parser.parse_args()

    success, failed = upload_directory(
        args.local_dir,
        args.remote_dir,
        WEBDAV_URL,
        WEBDAV_USER,
        WEBDAV_PASS
    )

    print()
    print("=" * 40)
    print(f"Upload complete: {success} succeeded, {failed} failed")
    print("=" * 40)

    sys.exit(0 if failed == 0 else 1)


if __name__ == "__main__":
    main()
