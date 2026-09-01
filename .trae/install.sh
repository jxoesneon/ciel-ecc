#!/bin/bash
#
# ECC Trae Installer
# Installs Everything Claude Code workflows into a Trae project.
#
# Usage:
#   ./install.sh              # Install to current directory
#   ./install.sh ~            # Install globally to ~/.trae/ or ~/.trae-cn/
#
# Environment:
#   TRAE_ENV=cn              # Force use .trae-cn directory
#

set -euo pipefail

# When globs match nothing, expand to empty list instead of the literal pattern
shopt -s nullglob

# Resolve the directory where this script lives (the repo root)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

# Get the trae directory name (.trae or .trae-cn)
get_trae_dir() {
    if [ "${TRAE_ENV:-}" = "cn" ]; then
        echo ".trae-cn"
    else
        echo ".trae"
    fi
}

# Install function
do_install() {
    local target_dir="$PWD"
    local trae_dir="$(get_trae_dir)"
    local commands=0 agents=0 skills=0 rules=0 other=0

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

    echo "ECC Trae Installer"
    echo "=================="
    echo ""
    echo "Source:  $REPO_ROOT"
    echo "Target:  $trae_full_path/"
    echo ""

    # Subdirectories to create
    SUBDIRS="commands agents skills rules"

    # Create all required trae subdirectories
    for dir in $SUBDIRS; do
        mkdir -p "$trae_full_path/$dir"
    done

    # Manifest file to track installed files
    MANIFEST="$trae_full_path/.ecc-manifest"

    python3 -c '
import os, sys, shutil

repo_root = os.path.realpath(sys.argv[1])
script_dir = os.path.realpath(sys.argv[2])
trae_full = os.path.realpath(sys.argv[3])

manifest_file = os.path.join(trae_full, ".ecc-manifest")
existing_manifest = set()
if os.path.exists(manifest_file):
    with open(manifest_file, "r", encoding="utf-8") as f:
        existing_manifest = set(line.strip() for line in f if line.strip())

new_manifest = set(existing_manifest)

def copy_file(src, rel_dest, is_exec=False):
    dst = os.path.join(trae_full, rel_dest)
    dest_exists = os.path.exists(dst)
    if dest_exists and rel_dest not in existing_manifest:
        return False
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copy2(src, dst)
    if is_exec:
        try:
            os.chmod(dst, 0o755)
        except OSError:
            pass
    new_manifest.add(rel_dest)
    return True

# Commands
cmd_dir = os.path.join(repo_root, "commands")
if os.path.isdir(cmd_dir):
    for f in os.listdir(cmd_dir):
        if f.endswith(".md"):
            copy_file(os.path.join(cmd_dir, f), os.path.join("commands", f))

# Agents
agent_dir = os.path.join(repo_root, "agents")
if os.path.isdir(agent_dir):
    for f in os.listdir(agent_dir):
        if f.endswith(".md"):
            copy_file(os.path.join(agent_dir, f), os.path.join("agents", f))

# Skills
skill_dir = os.path.join(repo_root, "skills")
if os.path.isdir(skill_dir):
    for root, _, files in os.walk(skill_dir):
        for f in files:
            src = os.path.join(root, f)
            rel = os.path.relpath(src, repo_root)
            copy_file(src, rel)

# Rules
rules_dir = os.path.join(repo_root, "rules")
if os.path.isdir(rules_dir):
    for root, _, files in os.walk(rules_dir):
        for f in files:
            src = os.path.join(root, f)
            rel = os.path.relpath(src, repo_root)
            copy_file(src, rel)

# Readmes and scripts
for name in ["README.md", "README.zh-CN.md"]:
    src = os.path.join(script_dir, name)
    if os.path.exists(src):
        copy_file(src, name)

for name in ["install.sh", "uninstall.sh"]:
    src = os.path.join(script_dir, name)
    if os.path.exists(src):
        copy_file(src, name, is_exec=True)

new_manifest.add(".ecc-manifest")

with open(manifest_file, "w", encoding="utf-8") as f:
    for entry in sorted(new_manifest):
        f.write(entry + "\n")
' "$REPO_ROOT" "$SCRIPT_DIR" "$trae_full_path"

    # Installation summary
    commands=$(find "$trae_full_path/commands" -name "*.md" 2>/dev/null | wc -l | tr -d ' ')
    agents=$(find "$trae_full_path/agents" -name "*.md" 2>/dev/null | wc -l | tr -d ' ')
    skills=$(find "$trae_full_path/skills" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
    rules=$(find "$trae_full_path/rules" -name "*.md" 2>/dev/null | wc -l | tr -d ' ')

    echo "Installation complete!"
    echo ""
    echo "Components installed:"
    echo "  Commands:  $commands"
    echo "  Agents:    $agents"
    echo "  Skills:    $skills"
    echo "  Rules:     $rules"
    echo ""
    echo "Directory:   $(basename "$trae_full_path")"
    echo ""
    echo "Next steps:"
    echo "  1. Open your project in Trae"
    echo "  2. Type / to see available commands"
    echo "  3. Enjoy the ECC workflows!"
    echo ""
    echo "To uninstall later:"
    echo "  cd $trae_full_path"
    echo "  ./uninstall.sh"
}

# Main logic
do_install "$@"
