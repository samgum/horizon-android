#!/usr/bin/env python3
"""Download pinned Minecraft 26.3 files from Mojang's public CDN for horizon-prepare in CI."""

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
from pathlib import Path
import sys
import time
import urllib.request

PISTON_META = "https://piston-meta.mojang.com/v1/packages"
RESOURCES_CDN = "https://resources.download.minecraft.net"


def sha1_file(path: Path) -> str:
    h = hashlib.sha1()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 256)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def fetch_verified(url: str, dest: Path, expected_sha1: str, retries: int = 4) -> None:
    if dest.is_file() and (not expected_sha1 or sha1_file(dest) == expected_sha1):
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    last_err = None
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "Horizon-CI/26.3"})
            with urllib.request.urlopen(req, timeout=45) as resp:
                data = resp.read()
            if expected_sha1:
                got = hashlib.sha1(data).hexdigest()
                if got != expected_sha1:
                    raise ValueError(f"sha1 mismatch for {url}: expected {expected_sha1}, got {got}")
            tmp.write_bytes(data)
            tmp.replace(dest)
            return
        except Exception as exc:  # noqa: BLE001
            last_err = exc
            time.sleep(0.6 * (attempt + 1))
    raise RuntimeError(f"failed to download {url}: {last_err}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", default="research-lock.json", help="Path to research-lock.json")
    parser.add_argument("--out", required=True, help="Target minecraft directory")
    parser.add_argument("--jobs", type=int, default=24, help="Parallel asset download workers")
    args = parser.parse_args()

    lock = json.loads(Path(args.lock).read_text(encoding="utf-8"))["minecraft"]
    version = lock["version"]
    manifest_sha1 = lock["manifest_sha1"]
    client_sha1 = lock["client_sha1"]
    server_sha1 = lock["server_sha1"]

    out = Path(args.out).resolve()
    manifest_path = out / "versions" / version / f"{version}.json"
    manifest_url = f"{PISTON_META}/{manifest_sha1}/{version}.json"
    print(f"[mojang] fetching version manifest {version} ({manifest_sha1})...")
    fetch_verified(manifest_url, manifest_path, manifest_sha1)

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    client_info = manifest["downloads"]["client"]
    server_info = manifest["downloads"]["server"]

    client_jar = out / "versions" / version / f"{version}.jar"
    server_jar = out / "server.jar"
    print(f"[mojang] fetching client.jar and server.jar for {version}...")
    fetch_verified(client_info["url"], client_jar, client_sha1)
    fetch_verified(server_info["url"], server_jar, server_sha1)

    lib_tasks = []
    for lib in manifest.get("libraries", []):
        dl = lib.get("downloads", {})
        art = dl.get("artifact")
        if art and art.get("url") and art.get("path"):
            lib_tasks.append((art["url"], out / "libraries" / art["path"], art.get("sha1", "")))
        for clf in (dl.get("classifiers") or {}).values():
            if clf and clf.get("url") and clf.get("path"):
                lib_tasks.append((clf["url"], out / "libraries" / clf["path"], clf.get("sha1", "")))

    print(f"[mojang] fetching {len(lib_tasks)} Maven libraries...")
    with ThreadPoolExecutor(max_workers=min(args.jobs, 16)) as pool:
        futures = [pool.submit(fetch_verified, u, p, s) for (u, p, s) in lib_tasks]
        for fut in as_completed(futures):
            fut.result()

    asset_idx = manifest["assetIndex"]
    idx_path = out / "assets" / "indexes" / f"{asset_idx['id']}.json"
    fetch_verified(asset_idx["url"], idx_path, asset_idx.get("sha1", ""))
    objects = json.loads(idx_path.read_text(encoding="utf-8")).get("objects", {})

    unique_hashes = sorted({entry["hash"] for entry in objects.values() if "hash" in entry})
    print(f"[mojang] fetching {len(unique_hashes)} unique asset objects...")

    def dl_obj(h: str) -> None:
        prefix = h[:2]
        fetch_verified(f"{RESOURCES_CDN}/{prefix}/{h}", out / "assets" / "objects" / prefix / h, h)

    done = 0
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = [pool.submit(dl_obj, h) for h in unique_hashes]
        for fut in as_completed(futures):
            fut.result()
            done += 1
            if done % 500 == 0 or done == len(unique_hashes):
                print(f"[mojang] assets: {done}/{len(unique_hashes)}")

    print(f"[mojang] {version} download complete at {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
