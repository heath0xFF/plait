# Plait artwork

This directory contains the vector kit supplied by the project owner. The kit
files are unchanged. See [README.txt](README.txt) for its provenance and editing
notes: these vectors reconstruct the reference image; they are not its original
source paths.

- `marks/plait-mark-gradient.svg`: sidebar ribbon mark, rendered in full color.
- `wordmarks/plait-wordmark-light.svg`: outlined sidebar lettering; no font needed.
- `icons/`: category glyphs. GPUI uses the project, machine, thread, and Git paths
  with the existing UI colors. Other glyphs remain available in the kit, but do
  not add controls for features that do not exist.
- `app-icon/plait-app-icon.svg`: editable app-icon master.
- `app-icon/plait-app-icon-1024.png`: supplied export used to generate the macOS
  icon set. When changing the master, export this PNG again at 1024 × 1024 with
  transparency before building the app.
- `gallery.html` and `plait-preview.*`: reference previews only, not app UI.

The desktop embeds its SVGs at compile time. It does not read this directory
at runtime. The app bundle contains the compiled `.icns`, not the whole kit.
