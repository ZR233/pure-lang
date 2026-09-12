#!/usr/bin/env python3
"""Register this relocatable bundle for the current Linux desktop user."""

import os
from pathlib import Path


def main():
    bundle = Path(__file__).resolve().parent
    executable = bundle / "anywork"
    icon = bundle / "share/icons/hicolor/1024x1024/apps/io.github.zr233.anywork.png"
    if not executable.is_file() or not icon.is_file():
        raise SystemExit("Run this script from a built anywork bundle.")
    # Desktop Entry string escaping precedes Exec argument unquoting.
    argument = str(executable).replace("%", "%%")
    for character in ("\\", '"', "`", "$"):
        argument = argument.replace(character, "\\" + character)
    command = ('"' + argument + '"').replace("\\", "\\\\")
    icon_value = str(icon).replace("\\", "\\\\")
    if any(character in str(bundle) for character in ("\n", "\r")):
        raise SystemExit("The bundle path must not contain line breaks.")
    entry = (bundle / "share/applications/io.github.zr233.anywork.desktop").read_text()
    # GIO checks the program before expanding %% in arguments; env lets bundle
    # paths containing a literal percent sign remain valid executable arguments.
    entry = entry.replace("Exec=anywork\n", f"Exec=env {command}\n")
    entry = entry.replace("Icon=io.github.zr233.anywork\n", f"Icon={icon_value}\n")
    data_home = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    destination = data_home / "applications/io.github.zr233.anywork.desktop"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(entry)
    print(destination)


if __name__ == "__main__":
    main()
