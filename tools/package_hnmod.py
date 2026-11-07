#!/usr/bin/env python3
"""
Automated packaging pipeline for Yefira Live Sync (.hnmod) targeting Horizon.
Generates an official, self-contained native mod bundle ready for drag-and-drop
into Horizon Launcher or installation into horizon-native-mods.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

DEFAULT_HORIZON_REV = "41f822b659789780f58d32c63ddfb6887ea1b8bf"
DEFAULT_MIXINS_REV = "5a64af938ec86b00d53d16a408aabc0038fb3e57"
DEFAULT_GAME = "26.3"
MOD_ID = "yefira"


def find_horizon_dir() -> Path | None:
    candidates = [
        Path("/Volumes/Data/code_projects/mtk/horizon"),
        Path(__file__).resolve().parent.parent.parent / "horizon",
    ]
    for c in candidates:
        if c.exists() and (c / "Cargo.toml").exists():
            return c
    return None


def get_git_commit(repo_dir: Path) -> str | None:
    try:
        out = subprocess.check_output(
            ["git", "-C", str(repo_dir), "rev-parse", "HEAD"],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        if len(out) == 40:
            return out
    except Exception:
        pass
    return None


def package_hnmod(
    yefira_root: Path,
    output_dir: Path,
    horizon_rev: str,
    mixins_rev: str,
    version: str,
    run_check: bool = True,
) -> Path:
    staging_dir = yefira_root / "target" / "hnmod_staging"
    if staging_dir.exists():
        shutil.rmtree(staging_dir)
    staging_dir.mkdir(parents=True, exist_ok=True)

    output_dir.mkdir(parents=True, exist_ok=True)
    hnmod_file = output_dir / f"{MOD_ID}-{version}.hnmod"

    print(f"📦 Packaging Yefira Native Mod v{version} for Horizon...")
    print(f"   • Horizon commit: {horizon_rev}")
    print(f"   • Mixins commit:  {mixins_rev}")

    # 1. Write horizon-native.json
    native_json = {
        "schema": 1,
        "id": MOD_ID,
        "version": version,
        "game": DEFAULT_GAME,
        "horizon": horizon_rev,
        "mixins": mixins_rev,
    }
    with open(staging_dir / "horizon-native.json", "w", encoding="utf-8") as f:
        json.dump(native_json, f, indent=2)

    # 2. Setup mixins/yefira directory structure
    mixins_dir = staging_dir / "mixins" / MOD_ID
    mixins_dir.mkdir(parents=True, exist_ok=True)

    # 2.1 horizon-mixin.toml
    mixin_toml = f"""[mod]
id = "{MOD_ID}"
version = "{version}"
priority = 100

[compatibility]
horizon_revision = "{horizon_rev}"

[api]
path = "api"
"""
    with open(mixins_dir / "horizon-mixin.toml", "w", encoding="utf-8") as f:
        f.write(mixin_toml)

    # 2.2 api/ package (Completely Self-Contained Horizon Trait Mod)
    api_dir = mixins_dir / "api"
    api_dir.mkdir(parents=True, exist_ok=True)
    api_src_dir = api_dir / "src"
    api_src_dir.mkdir(parents=True, exist_ok=True)

    api_cargo_toml = f"""[package]
name = "mod-{MOD_ID}"
version = "{version}"
edition = "2024"

[dependencies]
horizon-client = {{ git = "https://github.com/horizon-rust/horizon.git", rev = "{horizon_rev}" }}
glam = {{ version = "0.30", features = ["serde"] }}
xxhash-rust = {{ version = "0.8", features = ["xxh64"] }}
tungstenite = {{ version = "0.30", default-features = false }}
sha1 = "0.10"
base64 = "0.22"
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
parking_lot = "0.12"
thiserror = "2.0"
log = "0.4"
"""
    with open(api_dir / "Cargo.toml", "w", encoding="utf-8") as f:
        f.write(api_cargo_toml)

    # Copy canonical source tree from yefira_root/src/ into api/src/
    yefira_src = yefira_root / "src"
    for root, dirs, files in os.walk(yefira_src):
        rel_dir = Path(root).relative_to(yefira_src)
        target_dir = api_src_dir / rel_dir
        target_dir.mkdir(parents=True, exist_ok=True)
        for file in files:
            shutil.copy2(Path(root) / file, target_dir / file)

    # 3. Zip into .hnmod with deterministic Unix permissions (0o644)
    if hnmod_file.exists():
        hnmod_file.unlink()

    with zipfile.ZipFile(hnmod_file, "w", zipfile.ZIP_DEFLATED) as zf:
        for root, _, files in os.walk(staging_dir):
            for file in sorted(files):
                full_path = Path(root) / file
                rel_path = full_path.relative_to(staging_dir).as_posix()
                zinfo = zipfile.ZipInfo(rel_path)
                zinfo.external_attr = 0o100644 << 16  # standard regular file mode
                zinfo.compress_type = zipfile.ZIP_DEFLATED
                with open(full_path, "rb") as f_in:
                    zf.writestr(zinfo, f_in.read())

    # Clean up staging directory
    shutil.rmtree(staging_dir, ignore_errors=True)

    print(f"✅ Generated bundle archive: {hnmod_file} ({hnmod_file.stat().st_size} bytes)")

    # 4. Optional verification with cargo-horizon-mod check
    if run_check:
        horizon_dir = find_horizon_dir()
        if horizon_dir:
            print("🔍 Verifying .hnmod compliance with Horizon official mod checker...")
            cmd = [
                "cargo",
                "run",
                "--manifest-path",
                str(horizon_dir / "Cargo.toml"),
                "-p",
                "cargo-horizon-mod",
                "--",
                "check",
                str(hnmod_file),
            ]
            res = subprocess.run(cmd, capture_output=True, text=True)
            if res.returncode == 0:
                print(f"✨ Horizon Verification PASSED:\n   {res.stdout.strip()}")
            else:
                print(f"⚠️ Horizon check failed:\n{res.stderr.strip()}", file=sys.stderr)
                sys.exit(1)

    return hnmod_file


def main():
    parser = argparse.ArgumentParser(description="Package yefira-rs into an official Horizon .hnmod mod file.")
    parser.add_argument("--horizon", help="Pinned Horizon commit hash (40 hex chars)")
    parser.add_argument("--mixins", help="Pinned horizon-mixins commit hash (40 hex chars)", default=DEFAULT_MIXINS_REV)
    parser.add_argument("--version", help="Mod semver version", default="0.1.0")
    parser.add_argument("--out", help="Output directory", default="dist")
    parser.add_argument("--no-check", action="store_true", help="Skip running cargo-horizon-mod check")

    args = parser.parse_args()

    yefira_root = Path(__file__).resolve().parent.parent
    horizon_dir = find_horizon_dir()

    horizon_rev = args.horizon
    if not horizon_rev and horizon_dir:
        horizon_rev = get_git_commit(horizon_dir)
    if not horizon_rev:
        horizon_rev = DEFAULT_HORIZON_REV

    output_dir = yefira_root / args.out
    hnmod_path = package_hnmod(
        yefira_root=yefira_root,
        output_dir=output_dir,
        horizon_rev=horizon_rev,
        mixins_rev=args.mixins,
        version=args.version,
        run_check=not args.no_check,
    )

    print("\n🎉 Packaging completed successfully!")
    print(f"👉 Target File: {hnmod_path}")
    print("\nInstallation:")
    print("Drag & Drop this .hnmod file directly into Horizon Launcher (Mods tab), or copy to horizon-native-mods/.")


if __name__ == "__main__":
    main()
