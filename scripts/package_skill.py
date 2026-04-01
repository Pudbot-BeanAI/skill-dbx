#!/usr/bin/env python3

"""Build deterministic .skill archives for the dbx-dba skill."""

from __future__ import annotations

import argparse
import shutil
import stat
import tempfile
import zipfile
from pathlib import Path

FIXED_TIMESTAMP = (2024, 1, 1, 0, 0, 0)
SKIP_FILENAMES = {".gitkeep", ".DS_Store"}


def parse_args() -> argparse.Namespace:
    repo_root = Path(__file__).resolve().parent.parent

    parser = argparse.ArgumentParser(
        description="Package the dbx-dba skill with an embedded platform-specific dbx binary."
    )
    parser.add_argument(
        "--skill-dir",
        type=Path,
        default=repo_root / "skill",
        help="Path to the canonical flattened dbx-dba skill root.",
    )
    parser.add_argument(
        "--binary",
        type=Path,
        required=True,
        help="Path to the compiled dbx binary to embed into the skill package.",
    )
    parser.add_argument(
        "--target-os",
        choices=("linux", "macos", "windows"),
        required=True,
        help="Target operating system label used in the output artifact name.",
    )
    parser.add_argument(
        "--target-arch",
        default="amd64",
        help="Target architecture label used in the output artifact name.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=repo_root / "dist",
        help="Directory where the packaged .skill file will be written.",
    )
    parser.add_argument(
        "--skill-name",
        default="dbx-dba",
        help="Output artifact prefix.",
    )
    return parser.parse_args()


def should_skip(path: Path) -> bool:
    return path.name in SKIP_FILENAMES


def copy_skill_tree(source_dir: Path, target_dir: Path) -> None:
    shutil.copytree(
        source_dir,
        target_dir,
        ignore=shutil.ignore_patterns(*SKIP_FILENAMES),
    )


def normalized_mode(source: Path) -> int:
    source_mode = source.stat().st_mode
    return 0o755 if source_mode & 0o111 else 0o644


def add_file(archive: zipfile.ZipFile, archive_path: Path, source: Path, mode: int) -> None:
    info = zipfile.ZipInfo(str(archive_path).replace("\\", "/"), FIXED_TIMESTAMP)
    info.create_system = 3
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = ((stat.S_IFREG | mode) & 0xFFFF) << 16
    archive.writestr(info, source.read_bytes())


def iter_skill_files(skill_dir: Path):
    for source in sorted(skill_dir.rglob("*")):
        if source.is_dir() or should_skip(source):
            continue

        yield source.relative_to(skill_dir), source


def embedded_binary_name(target_os: str) -> str:
    return "dbx.exe" if target_os == "windows" else "dbx"


def stage_skill(skill_dir: Path, binary: Path, target_os: str, skill_name: str) -> Path:
    staging_root = Path(tempfile.mkdtemp(prefix="dbx-skill-"))
    staged_skill_dir = staging_root / skill_name
    copy_skill_tree(skill_dir, staged_skill_dir)

    staged_binary = staged_skill_dir / "assets" / "bin" / embedded_binary_name(target_os)
    staged_binary.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(binary, staged_binary)
    staged_binary.chmod(0o755)

    return staged_skill_dir


def build_archive(staged_skill_dir: Path, output_dir: Path, target_os: str, target_arch: str, skill_name: str) -> Path:
    archive_path = output_dir / f"{skill_name}-{target_os}-{target_arch}.skill"

    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for relative_path, source in iter_skill_files(staged_skill_dir):
            add_file(archive, relative_path, source, normalized_mode(source))

    return archive_path


def main() -> int:
    args = parse_args()
    skill_dir = args.skill_dir.resolve()
    binary = args.binary.resolve()
    output_dir = args.output_dir.resolve()

    if not (skill_dir / "SKILL.md").is_file():
        raise SystemExit(f"skill directory is missing SKILL.md: {skill_dir}")

    if not binary.is_file():
        raise SystemExit(f"embedded binary does not exist: {binary}")

    output_dir.mkdir(parents=True, exist_ok=True)

    staged_skill_dir = stage_skill(skill_dir, binary, args.target_os, args.skill_name)
    try:
        archive_path = build_archive(
            staged_skill_dir=staged_skill_dir,
            output_dir=output_dir,
            target_os=args.target_os,
            target_arch=args.target_arch,
            skill_name=args.skill_name,
        )
    finally:
        shutil.rmtree(staged_skill_dir.parent, ignore_errors=True)

    print(archive_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
