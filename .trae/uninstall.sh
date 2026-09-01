#!/bin/bash
#
# ECC Trae Uninstaller
# Uninstalls Everything Claude Code workflows from a Trae project.
#
# Usage:
#   ./uninstall.sh              # Uninstall from current directory
#   ./uninstall.sh ~            # Uninstall globally from ~/.trae/
#
# Environment:
#   TRAE_ENV=cn              # Force use .trae-cn directory
#

set -euo pipefail

# Resolve the directory where this script lives
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Get the trae directory name (.trae or .trae-cn)
get_trae_dir() {
    # Check environment variable first
    if [ "${TRAE_ENV:-}" = "cn" ]; then
        echo ".trae-cn"
    else
        echo ".trae"
    fi
}

resolve_path() {
    python3 -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$1"
}

is_valid_manifest_entry() {
    local file_path="$1"

    case "$file_path" in
        ""|/*|~*|*/../*|../*|*/..|..)
            return 1
            ;;
    esac

    return 0
}

# Main uninstall function
do_uninstall() {
    local target_dir="$PWD"
    local trae_dir="$(get_trae_dir)"
    
    # Check if ~ was specified (or expanded to $HOME)
    if [ "$#" -ge 1 ]; then
        if [ "$1" = "~" ] || [ "$1" = "$HOME" ]; then
            target_dir="$HOME"
        fi
    fi
    
    # Check if we're already inside a .trae or .trae-cn directory
    local current_dir_name="$(basename "$target_dir")"
    local trae_full_path
    
    if [ "$current_dir_name" = ".trae" ] || [ "$current_dir_name" = ".trae-cn" ]; then
        # Already inside the trae directory, use it directly
        trae_full_path="$target_dir"
    else
        # Normal case: append trae_dir to target_dir
        trae_full_path="$target_dir/$trae_dir"
    fi
    
    echo "ECC Trae Uninstaller"
    echo "===================="
    echo ""
    echo "Target:  $trae_full_path/"
    echo ""
    
    if [ ! -d "$trae_full_path" ]; then
        echo "Error: $trae_dir directory not found at $target_dir"
        exit 1
    fi
    
    trae_root_resolved="$(resolve_path "$trae_full_path")"

    # Manifest file path
    MANIFEST="$trae_full_path/.ecc-manifest"
    
    if [ ! -f "$MANIFEST" ]; then
        echo "Warning: No manifest file found (.ecc-manifest)"
        echo ""
        echo "This could mean:"
        echo "  1. ECC was installed with an older version without manifest support"
        echo "  2. The manifest file was manually deleted"
        echo ""
        read -p "Do you want to remove the entire $trae_dir directory? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            echo "Uninstall cancelled."
            exit 0
        fi
        rm -rf "$trae_full_path"
        echo "Uninstall complete!"
        echo ""
        echo "Removed: $trae_full_path/"
        exit 0
    fi
    
    echo "Found manifest file - will only remove files installed by ECC"
    echo ""
    read -p "Are you sure you want to uninstall ECC from $trae_dir? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Uninstall cancelled."
        exit 0
    fi
    
    # Perform uninstallation via Python engine
    python3 -c '
import os, sys

trae_full = os.path.realpath(sys.argv[1])
trae_dir = sys.argv[2]
manifest_file = os.path.join(trae_full, ".ecc-manifest")

if not os.path.isdir(trae_full):
    print("Uninstall complete!\n")
    sys.exit(0)

if not os.path.exists(manifest_file):
    sys.exit(0)

entries = []
with open(manifest_file, "r", encoding="utf-8") as f:
    for line in f:
        e = line.strip()
        if e and e != ".ecc-manifest":
            entries.append(e)

removed = 0
skipped = 0

for entry in entries:
    if ".." in entry or entry.startswith("/") or entry.startswith("~"):
        print(f"Skipped: {entry} (invalid manifest entry)")
        skipped += 1
        continue
    full = os.path.join(trae_full, entry)
    try:
        real = os.path.realpath(full)
    except Exception:
        print(f"Skipped: {entry} (invalid manifest entry)")
        skipped += 1
        continue
    if real != trae_full and not real.startswith(trae_full + os.sep):
        print(f"Skipped: {entry} (invalid manifest entry)")
        skipped += 1
        continue
    if os.path.isfile(real) or os.path.islink(real):
        os.remove(real)
        print(f"Removed: {entry}")
        removed += 1
    else:
        skipped += 1

if os.path.exists(manifest_file):
    os.remove(manifest_file)
    print("Removed: .ecc-manifest")
    removed += 1

# Clean up empty directories bottom-up
for root, dirs, files in os.walk(trae_full, topdown=False):
    for d in dirs:
        dp = os.path.join(root, d)
        try:
            if not os.listdir(dp):
                os.rmdir(dp)
                rel = os.path.relpath(dp, trae_full)
                print(f"Removed: {rel}/")
                removed += 1
        except OSError:
            pass

try:
    if not os.listdir(trae_full):
        os.rmdir(trae_full)
        print(f"Removed: {trae_dir}/")
        removed += 1
except OSError:
    pass

print("")
print("Uninstall complete!")
print("")
print("Summary:")
print(f"  Removed: {removed} items")
print(f"  Skipped: {skipped} items (not found or user-modified)")
print("")
if os.path.isdir(trae_full):
    print(f"Note: {trae_dir} directory still exists (contains user-added files)")
' "$trae_full_path" "$trae_dir"
}

# Execute uninstall
do_uninstall "$@"
