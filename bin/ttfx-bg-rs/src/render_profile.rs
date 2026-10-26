//! One product switch. Trial optimization variables are deliberately not read.
//! The selected value is copied to the grid and pipeline at layer startup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderProfile {
    #[default]
    Reference,
    Clipped,
    Axis,
}
impl RenderProfile {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("clipped") => Self::Clipped,
            Some("axis") => Self::Axis,
            _ => Self::Reference,
        }
    }
    pub fn from_env() -> Self {
        Self::parse(std::env::var("TTFX_BG_RENDER_PROFILE").ok().as_deref())
    }
    pub fn name(self) -> &'static str {
        match self { Self::Reference => "reference", Self::Clipped => "clipped", Self::Axis => "axis" }
    }
    pub fn cached(self) -> bool { self != Self::Reference }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn absent_invalid_and_explicit_reference_select_original() {
        for v in [None, Some(""), Some("reference"), Some("AXIS"), Some("axis "), Some("auto"), Some("1")] {
            assert_eq!(RenderProfile::parse(v), RenderProfile::Reference);
            assert!(!RenderProfile::parse(v).cached());
        }
        assert_eq!(RenderProfile::parse(Some("clipped")), RenderProfile::Clipped);
        assert_eq!(RenderProfile::parse(Some("axis")), RenderProfile::Axis);
    }
}

// Audit/test list only, not read on the product path. All are ignored by v1.
#[cfg(test)]
pub const LEGACY_TRIALS:&[&str]=&[
    "TTFX_BG_NATIVE_SWAP","TTFX_BG_NATIVE_SGR_CACHE","TTFX_BG_GPU_GLYPH_SLOT_CACHE",
    "TTFX_BG_GPU_FAST_GLYPHS","TTFX_BG_GPU_CLIPPED_QUADS","TTFX_BG_GPU_CELL_QUADS",
    "TTFX_BG_GPU_AXIS_LOOKUP","TTFX_BG_GPU_UPLOAD_DIRTY","TTFX_BG_GPU_QUAD_STRIP",
    "TTFX_BG_GPU_CELL_PLANES","TTFX_BG_GPU_FLAT_STATE","TTFX_BG_GPU_SPARSE_CELLS",
    "TTFX_BG_WL_DAMAGE_CACHE","TTFX_BG_NATIVE_TIMELINE","TTFX_BG_NATIVE_COLOR_REUSE",
    "TTFX_BG_GPU_GLYPH_BOUNDS","TTFX_BG_NATIVE_TINY_SGR","TTFX_BG_GPU_ATLAS_BITS",
    "TTFX_BG_GPU_ZERO_BG","TTFX_BG_GPU_EXACT_CLIP_BATCH","TTFX_BG_GPU_TEXEL_PAIRS",
    "TTFX_BG_GPU_AXIS_16BIT","TTFX_BG_GPU_TILE_DAMAGE","TTFX_BG_NATIVE_LAZY_TYPED_RGB",
    "TTFX_BG_GPU_OPAQUE_PROGRAM","TTFX_BG_WL_DEFER_CONSUME",
];
