"""Notes' Android launcher icon: the desktop's icon script draws it (the GNOME-style legal pad edge
to edge as the adaptive background, an empty foreground, and the flat icon for launchers before
Android 8), so both apps share one drawing.
usage: adaptive_icon.py <android res dir>"""
import os
import subprocess
import sys

script = os.path.expanduser("~/CascadeProjects/notes-linux/tools/icon.py")
sys.exit(subprocess.call([sys.executable, script, "android", sys.argv[1]]))
