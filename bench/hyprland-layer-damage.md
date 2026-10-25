# Hyprland repaints the whole layer on every layer-surface commit

Source: Hyprland v0.56.2, `src/desktop/view/LayerSurface.cpp`, `CLayerSurface::onCommit()`.
Once the layer is mapped, every commit runs:

```cpp
if (m_layer == ZWLR_LAYER_SHELL_V1_LAYER_BACKGROUND || m_layer == ZWLR_LAYER_SHELL_V1_LAYER_BOTTOM)
    PMONITOR->m_blurFBDirty = true;

CBox geomFixed = {m_geometry.x, m_geometry.y, m_geometry.width, m_geometry.height};
g_pHyprRenderer->damageBox(geomFixed);
```

before looking at the committed state. The surface damage the client sent
(`wl_surface.damage_buffer` / `eglSwapBuffersWithDamage`) is ignored for the
purpose of output damage: the full layer geometry is damaged on every commit.
Toplevel windows go through `damageSurface()` instead and keep per-surface damage.

Consequences for an animated full-screen background layer:

- The compositor recomposites the whole output per commit even when one cell
  changed. Measured on mbp-intel (2560x1600, Iris 6100) at 30 fps: reporting
  1 px of damage, the bounding box, or full damage all gave the same
  Hyprland CPU (~7%) and GPU busy (~36%).
- With blur enabled, the blur framebuffer is marked dirty on every commit of a
  background/bottom layer, so the blur is recomputed at the layer's frame rate.
- The only client-side lever is the number of commits (frame rate).

Possible upstream fix: replace `damageBox(geomFixed)` with
`damageSurface(m_wlSurface->resource(), geometry.x, geometry.y)` (as windows do)
and only set `m_blurFBDirty` when the committed damage is non-empty, or when
the surface actually changed size/position. Needs care for the first commit
after a resize (full damage is still right there).

Repro data: `bench/results4.txt` (m-normal vs m-nodraw vs m-fullclip) and
`results2.txt` rows matrix-min / matrix-full from 2026-10-25 on mbp-intel.
