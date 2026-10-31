# Ambient live selector preview

`ttfx-ambient-preview --preview-daemon --ambient-preview` supplies JSON-lines
frames for an ASCII background selector. The older preview-daemon mode remains
available. Ambient mode uses a fixed producer config and literal Omarchy branding,
not a resampled wordmark. Native previews share one playback-monitor analyzer, and
catalog previews also publish volume for the UI's brightness response.
Interactive scenes run their actual models, with global cursor tracking.

The selected slice animates and its neighbours show posters. Custom eye background
cells are serialized as coloured block cells and painted as rectangles. A
process-group wrapper owns the preview and analyzer children, so closing the panel
leaves the active wallpaper alone. Applying a choice is delegated to Ambient.

The example QML grid uses integer physical cell units, aligned global origins and
nearest texture sampling. It preserves cell aspect rather than stretching into the
card. Caption colours have a visible fallback when the shell has no imagePicker
theme group.
