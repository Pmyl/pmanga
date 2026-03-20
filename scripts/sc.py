#!/usr/bin/env python3
"""
Fetches every One Piece chapter with its volume number from
  https://api.api-onepiece.com/v2/chapters/en
and writes:
  one_piece_chapters.csv  (manga_title, chapter_number, tankobon_number)

Requirements: Python 3.7+, no extra packages (uses stdlib only)
Usage:        python3 fetch_one_piece_chapters.py
"""

import csv
import json
import sys
import time
import urllib.error
import urllib.request

BASE_URL = "https://api.api-onepiece.com/v2/chapters/en"
PAGE_SIZE = 200  # ask for 200 per request to minimise round-trips
OUTPUT_CSV = "one_piece_chapters.csv"


def fetch_page(offset: int, limit: int) -> list[dict]:
    url = f"{BASE_URL}?limit={limit}&offset={offset}"
    req = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read().decode())


def main() -> None:
    chapters: list[dict] = []
    offset = 0

    print("Fetching chapters from api.api-onepiece.com …")
    while True:
        try:
            page = fetch_page(offset, PAGE_SIZE)
        except urllib.error.URLError as exc:
            sys.exit(f"Network error: {exc}")

        if not page:  # empty page → we're done
            break

        chapters.extend(page)
        print(f"  fetched {len(chapters)} chapters so far …")

        if len(page) < PAGE_SIZE:  # last (partial) page
            break

        offset += PAGE_SIZE
        time.sleep(0.3)  # be polite

    if not chapters:
        sys.exit("No chapters returned — check the API URL or your connection.")

    # Sort by chapter number (field name in the API is "number")
    # The API returns: {"id": 1, "title": "…", "volume": 1, "number": 1, …}
    # Some older endpoints use "chapter" instead of "number" — handle both.
    def chapter_key(ch: dict) -> int:
        return int(ch.get("number") or ch.get("chapter") or 0)

    chapters.sort(key=chapter_key)

    # Write CSV
    with open(OUTPUT_CSV, "w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f)
        writer.writerow(["manga_title", "chapter_number", "tankobon_number"])

        for ch in chapters:
            num = ch.get("number") or ch.get("chapter")
            vol = ch.get("volume")
            if num is None or vol is None:
                print(f"  WARNING: skipping malformed entry: {ch}")
                continue
            writer.writerow(["One Piece", int(num), int(vol)])

    print(f"\nDone — {len(chapters)} rows written to '{OUTPUT_CSV}'")

    # Quick sanity-check: report chapter ranges per volume
    from collections import defaultdict

    vol_map: dict[int, list[int]] = defaultdict(list)
    for ch in chapters:
        num = ch.get("number") or ch.get("chapter")
        vol = ch.get("volume")
        if num and vol:
            vol_map[int(vol)].append(int(num))

    print(f"\nVolume summary ({len(vol_map)} volumes):")
    for v in sorted(vol_map):
        chs = sorted(vol_map[v])
        print(f"  Vol {v:>4}: ch {chs[0]:>5} – {chs[-1]:>5}  ({len(chs)} chapters)")


if __name__ == "__main__":
    main()
