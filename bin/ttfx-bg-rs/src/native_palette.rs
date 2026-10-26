//! Cache exacta de la paleta nativa; no interpreta un SGR diferente del baseline.
pub fn resolve(palette: &[String]) -> Vec<Option<(u8,u8,u8)>> {
    if palette.is_empty() { return vec![crate::ansi_sgr_to_rgb("\x1b[32m")]; }
    palette.iter().map(|s| crate::ansi_sgr_to_rgb(if s.is_empty(){"\x1b[32m"}else{s})).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_cache_matches_baseline_fallbacks_and_parser_exactly() {
        let fixtures=vec![vec![],vec![""],vec!["\x1b[31m","\x1b[38;5;196m","\x1b[38;2;1;2;3m"],
            vec!["\x1b[0m","\x1b[97m","garbage","\x1b[39m"],vec!["\x1b[1;32m","\x1b[38;2;255;0;0m\x1b[34m"]];
        for palette in fixtures {
            let palette:Vec<String>=palette.into_iter().map(str::to_string).collect();
            let cached=resolve(&palette);
            for index in 0..8 {
                let idx=index.min(palette.len().saturating_sub(1));
                let sgr=palette.get(idx).filter(|s|!s.is_empty()).map(|s|s.as_str()).unwrap_or("\x1b[32m");
                assert_eq!(cached[idx],crate::ansi_sgr_to_rgb(sgr));
            }
        }
    }
}
