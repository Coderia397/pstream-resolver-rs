#!/usr/bin/env python3
"""
P-Stream Frontend File Tree Generator
--------------------------------------
Run this from anywhere — it always scans its own folder:
    python update_file_tree.py

Or point it at a specific folder:
    python update_file_tree.py /path/to/pstream-frontend

Updates file_tree_frondend.txt automatically every time. Each text/code file
is annotated with its line count; a summary breakdown by extension (file
count + total lines) is appended at the end.
"""

import sys
from pathlib import Path
from datetime import datetime

# ──────────────────────────────────────────────
# CONFIGURATION — edit these as your project grows
# ──────────────────────────────────────────────

OUTPUT_FILE = "file_tree_frondend.txt"

# Directories to skip entirely
EXCLUDED_DIRS = {
    "node_modules",
    "dist",
    "build",
    "out",
    ".git",
    ".svn",
    "__pycache__",
    ".vite",
    ".cache",
    ".turbo",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "coverage",
    ".nyc_output",
    "storybook-static",
}

# Specific filenames to skip
EXCLUDED_FILES = {
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
}

# File extensions to skip (generated / noisy output)
EXCLUDED_EXTENSIONS = {
    ".map",      # source maps
    ".log",      # log files
    ".tsbuildinfo",
}

# Extensions that are never text — don't even try to decode/count these.
# Anything not in this list is attempted as UTF-8 text; a real binary that
# slips through just fails the decode and is reported as "(binary)".
BINARY_EXTENSIONS = {
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".bmp", ".avif",
    ".woff", ".woff2", ".ttf", ".otf", ".eot",
    ".mp4", ".mov", ".webm", ".mp3", ".wav", ".ogg",
    ".zip", ".gz", ".tar", ".7z", ".rar",
    ".pdf", ".psd", ".ai", ".sketch",
    ".db", ".sqlite", ".sqlite3",
    ".pyc", ".class", ".exe", ".dll", ".so", ".dylib",
}

# ──────────────────────────────────────────────


def should_exclude(entry: Path) -> bool:
    if entry.is_dir():
        return entry.name in EXCLUDED_DIRS
    if entry.is_file():
        if entry.name in EXCLUDED_FILES:
            return True
        if entry.suffix in EXCLUDED_EXTENSIONS:
            return True
    return False


def count_lines(entry: Path) -> int | None:
    """Returns the line count for a text file, or None if it's binary/unreadable."""
    if entry.suffix.lower() in BINARY_EXTENSIONS:
        return None
    try:
        with entry.open("r", encoding="utf-8") as f:
            return sum(1 for _ in f)
    except (UnicodeDecodeError, PermissionError, OSError):
        return None


def human_size(num_bytes: int) -> str:
    size = float(num_bytes)
    for unit in ("B", "KB", "MB", "GB"):
        if size < 1024:
            return f"{size:.0f}{unit}" if unit == "B" else f"{size:.1f}{unit}"
        size /= 1024
    return f"{size:.1f}TB"


def build_tree(root_path: Path, prefix: str, stats: dict) -> list:
    lines = []

    try:
        raw_entries = list(root_path.iterdir())
    except PermissionError:
        return lines

    # Dirs first, then files — both sorted alphabetically
    entries = sorted(
        [e for e in raw_entries if not should_exclude(e)],
        key=lambda e: (e.is_file(), e.name.lower()),
    )

    for i, entry in enumerate(entries):
        is_last = i == len(entries) - 1
        connector = "└── " if is_last else "├── "

        if entry.is_dir():
            lines.append(f"{prefix}{connector}{entry.name}/")
            extension = "    " if is_last else "│   "
            lines.extend(build_tree(entry, prefix + extension, stats))
            stats["dirs"] += 1
        else:
            n_lines = count_lines(entry)
            ext = entry.suffix.lower() or "(no ext)"
            bucket = stats["by_ext"].setdefault(ext, {"files": 0, "lines": 0})
            bucket["files"] += 1
            stats["files"] += 1

            if n_lines is not None:
                bucket["lines"] += n_lines
                stats["total_lines"] += n_lines
                annotation = f"  ({n_lines:,} lines)"
            else:
                try:
                    annotation = f"  ({human_size(entry.stat().st_size)})"
                except OSError:
                    annotation = ""

            lines.append(f"{prefix}{connector}{entry.name}{annotation}")

    return lines


def build_extension_summary(by_ext: dict) -> list:
    lines = ["", "# Breakdown by extension (files, lines of code):"]
    # Sort by total lines descending — the extensions that matter most float to the top.
    ranked = sorted(by_ext.items(), key=lambda kv: kv[1]["lines"], reverse=True)
    for ext, data in ranked:
        if data["lines"] > 0:
            lines.append(f"#   {ext:<12} {data['files']:>4} files   {data['lines']:>7,} lines")
        else:
            lines.append(f"#   {ext:<12} {data['files']:>4} files")
    return lines


def main():
    # Accept optional path argument; default to the folder this script lives
    # in (not the shell's CWD) — running it from the parent `pstream/` repo
    # was scanning that parent instead of pstream-frontend.
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parent

    if not root.exists():
        print(f"Error: path not found — {root}")
        sys.exit(1)

    print(f"Scanning: {root}")

    stats = {"dirs": 0, "files": 0, "total_lines": 0, "by_ext": {}}
    tree_lines = build_tree(root, "", stats)

    header = [
        "# P-Stream Frontend — File Tree",
        f"# Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        f"# Root: {root}",
        "",
        root.name + "/",
    ]

    footer = [
        "",
        f"# {stats['dirs']} directories, {stats['files']} files, {stats['total_lines']:,} total lines of code",
    ]
    footer.extend(build_extension_summary(stats["by_ext"]))

    output = "\n".join(header + tree_lines + footer)

    output_path = root / OUTPUT_FILE
    output_path.write_text(output, encoding="utf-8")

    print(f"✓  Written to {output_path}")
    print(f"   {stats['dirs']} directories, {stats['files']} files, {stats['total_lines']:,} total lines")


if __name__ == "__main__":
    main()
