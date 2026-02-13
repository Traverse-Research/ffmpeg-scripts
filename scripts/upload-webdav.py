#!/usr/bin/env python3
"""
Upload a file to WebDAV server.

Usage: ./upload-webdav.py <local_file> <remote_path> --url <webdav_url> --user <username> --pass <password>

Example:
  ./upload-webdav.py video.mp4 /processed/video.mp4 --url https://nextcloud.example.com/remote.php/webdav --user admin --pass secret
"""

import argparse
import os
import sys
from pathlib import Path

import requests
from requests.auth import HTTPBasicAuth


def upload_file(local_path: str, remote_path: str, webdav_url: str, username: str, password: str) -> bool:
    """Upload a file to WebDAV server."""

    if not Path(local_path).exists():
        print(f"Error: File not found: {local_path}")
        return False

    # Build the full URL
    webdav_url = webdav_url.rstrip('/')
    remote_path = remote_path.lstrip('/')
    url = f"{webdav_url}/{remote_path}"

    file_size = Path(local_path).stat().st_size
    print(f"Uploading: {local_path}")
    print(f"Size: {file_size / (1024*1024):.1f} MB")
    print(f"To: {url}")
    print()

    try:
        with open(local_path, 'rb') as f:
            response = requests.put(
                url,
                data=f,
                auth=HTTPBasicAuth(username, password),
                headers={'Content-Type': 'application/octet-stream'},
            )

        if response.status_code in (200, 201, 204):
            print("Upload successful!")
            return True
        else:
            print(f"Upload failed: {response.status_code} {response.reason}")
            print(response.text[:500] if response.text else "")
            return False

    except Exception as e:
        print(f"Error: {e}")
        return False


def main():
    parser = argparse.ArgumentParser(description="Upload a file to WebDAV server")
    parser.add_argument("local_file", help="Local file to upload")
    parser.add_argument("remote_path", help="Remote path (e.g., /processed/video.mp4)")
    parser.add_argument("--url", required=True, help="WebDAV URL")
    parser.add_argument("--user", required=True, help="Username")
    parser.add_argument("--pass", dest="password", required=True, help="Password")

    args = parser.parse_args()

    success = upload_file(
        args.local_file,
        args.remote_path,
        args.url,
        args.user,
        args.password
    )

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
