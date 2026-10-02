#!/usr/bin/env python3
"""Verify reviewed local patches, then deny all RustSec vulnerabilities/unsoundness."""
import argparse
import hashlib
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PATCHED_HASH = "a0f5ee8acb8faa089bcdfbc9a57372609fce7654026ccef7d9a224d05a654ccc"
ADVISORY = "RUSTSEC-2024-0429"


def require(condition, message):
    if not condition:
        raise SystemExit("Dependency source verification failed: " + message)


def section(text, name):
    # These deliberately restrictive checks accept the reviewed manifest form,
    # rather than attempting to implement a permissive general TOML parser.
    matches = list(re.finditer(r"(?m)^\[" + re.escape(name) + r"\]\s*$", text))
    require(len(matches) == 1, "expected one " + name + " section")
    remainder = text[matches[0].end():]
    return re.split(r"(?m)^\[", remainder, maxsplit=1)[0]


def field(text, name):
    values = re.findall(r'(?m)^' + re.escape(name) + r'\s*=\s*"([^"\n]+)"\s*$', text)
    require(len(values) == 1, "expected one string field " + name)
    return values[0]


def verify_backport():
    patches = section((ROOT / "Cargo.toml").read_text(), "patch.crates-io")
    require(bool(re.search(r'(?m)^glib\s*=\s*\{\s*path\s*=\s*"vendor/glib"\s*\}\s*$', patches)), "glib patch must point to vendor/glib")
    package = section((ROOT / "vendor/glib/Cargo.toml").read_text(), "package")
    require(field(package, "name") == "glib" and field(package, "version") == "0.18.5", "unexpected vendored package identity")
    blocks = (ROOT / "Cargo.lock").read_text().split("[[package]]")
    glib = [block for block in blocks[1:] if re.search(r'(?m)^name = "glib"$', block)]
    require(len(glib) == 1, "expected a single glib package")
    require(field(glib[0], "version") == "0.18.5", "unexpected locked glib version")
    require(not re.search(r"(?m)^source\s*=", glib[0]), "glib is resolved from a registry instead of the reviewed path")
    source = ROOT / "vendor/glib/src/variant_iter.rs"
    require(hashlib.sha256(source.read_bytes()).hexdigest() == PATCHED_HASH, "backported VariantStrIter source changed")
    print(ADVISORY + ": verified glib 0.18.5 path patch and reviewed mutable-out-pointer backport", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--verify-only", action="store_true")
    options = parser.parse_args()
    verify_backport()
    if not options.verify_only:
        # RustSec skips path dependencies. The mandatory source check above
        # ensures the local GLib dependency still contains the reviewed fix.
        subprocess.run(["cargo", "audit", "--deny", "unsound"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
