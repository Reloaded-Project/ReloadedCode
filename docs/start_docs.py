#!/usr/bin/env python3
"""
Script to set up virtual environment and start MkDocs live server for documentation.

Usage:
    python start_docs.py                  # Start docs server
    python start_docs.py --docs-dir .     # Same as above (explicit)
"""

import subprocess
import sys
import os
from pathlib import Path


def run_command(cmd, cwd=None):
    """Run a command and handle errors."""
    print(f"Running: {' '.join(cmd)}")
    try:
        result = subprocess.run(cmd, cwd=cwd, check=True, capture_output=False)
        return result
    except subprocess.CalledProcessError as e:
        print(f"Error running command: {e}")
        sys.exit(1)


def main():
    """Main function to set up docs environment."""
    import argparse

    parser = argparse.ArgumentParser(description="Set up documentation environment")
    parser.add_argument(
        "--docs-dir",
        type=str,
        help="Documentation directory containing mkdocs.yml (default: script directory)",
    )
    parser.add_argument(
        "--project-name",
        type=str,
        default="reloaded-code",
        help="Project name for messages",
    )
    args = parser.parse_args()

    # Use docs directory if provided, otherwise use script directory
    if args.docs_dir:
        script_dir = Path(args.docs_dir)
    else:
        script_dir = Path(__file__).parent

    venv_dir = script_dir / "venv"

    print(f"Setting up {args.project_name} documentation...")

    # Create virtual environment if it doesn't exist
    if not venv_dir.exists():
        print("Creating virtual environment...")
        run_command([sys.executable, "-m", "venv", "venv"], cwd=script_dir)
    else:
        print("Virtual environment already exists.")

    # Determine the python executable in the venv
    if os.name == "nt":  # Windows
        python_exe = venv_dir / "Scripts" / "python.exe"
        pip_exe = venv_dir / "Scripts" / "pip.exe"
    else:  # Unix-like
        python_exe = venv_dir / "bin" / "python"
        pip_exe = venv_dir / "bin" / "pip"

    # Install required packages
    print("Installing required packages...")

    # Look for requirements.txt in the docs directory
    requirements_file = script_dir / "requirements.txt"
    if requirements_file.exists():
        print(f"Installing from {requirements_file}...")
        run_command(
            [str(pip_exe), "install", "-r", str(requirements_file)], cwd=script_dir
        )
    else:
        print(f"Warning: No requirements.txt found at {requirements_file}")

    # Start MkDocs live server
    print("Starting MkDocs live server...")
    print(
        "Documentation will be available at http://127.0.0.1:8000 (paste into browser address bar)"
    )
    print("Press Ctrl+C to stop the server")
    run_command(
        [str(python_exe), "-m", "mkdocs", "serve", "--livereload"], cwd=script_dir
    )


if __name__ == "__main__":
    main()
