use clap::{CommandFactory, Parser};
use ttfx::engine::ctx::{Clock, EngineCtx};
use ttfx::engine::terminal::TerminalConfig;
use ttfx::utils::geometry::Coord;
use ttfx::utils::graphics::{Color, Gradient, GradientDirection};
use ttfx::utils::rng::Rng;

fn palette() -> Gradient {
    let stops = ["112244", "44aacc", "ffeedd"].map(|hex| Color::from_hex(hex).unwrap());
    Gradient::with_steps(&stops, 12, false).unwrap()
}

fn context(height: usize) -> EngineCtx {
    let config = TerminalConfig {
        canvas_width: 40,
        canvas_height: height as i64 + 10,
        ignore_terminal_dimensions: true,
        ..Default::default()
    };
    // Include leading blank padding, like the plugin's centered text canvas.
    let input = format!("\n\n{}", vec!["    ABCD"; height].join("\n"));
    EngineCtx::new(
        &input,
        config,
        Rng::seeded(42),
        Clock::virtual_with_frame_rate(60),
    )
    .unwrap()
}

#[test]
fn scaled_bands_use_rounded_text_boundaries_and_native_spectrum() {
    let gradient = palette();
    let original = gradient.spectrum.clone();
    for (height, expected) in [
        (5, [1, 1, 1, 1, 1]),
        (10, [2, 2, 2, 1, 3]),
        (19, [4, 3, 4, 3, 5]),
        (38, [8, 6, 8, 6, 10]),
    ] {
        let mut ctx = context(height);
        ctx.final_text_bands = true;
        let map = ctx
            .final_gradient_mapping(&gradient, GradientDirection::Radial)
            .unwrap();
        let canvas = &ctx.terminal.canvas;
        assert_eq!(canvas.text_top - canvas.text_bottom + 1, height as i64);
        let mut row = canvas.text_top;
        for (band, count) in expected.into_iter().enumerate() {
            let color = gradient.get_color_at_fraction(band as f64 / 4.0).unwrap();
            for _ in 0..count {
                for column in canvas.text_left..=canvas.text_right {
                    assert_eq!(map.get(&Coord::new(column, row)), Some(color));
                }
                row -= 1;
            }
        }
        assert_eq!(row, canvas.text_bottom - 1);
        assert!(map
            .get(&Coord::new(canvas.text_left, canvas.text_top + 1))
            .is_none());
    }
    assert_eq!(
        gradient.spectrum, original,
        "animation spectrum was modified"
    );
}

#[test]
fn tiny_heights_single_color_and_invalid_bounds_are_safe() {
    let gradient = palette();
    for height in 1..=128 {
        let mapping = gradient
            .build_banded_coordinate_color_mapping(1, height, 1, 2)
            .unwrap();
        assert_eq!(mapping.iter().count(), (height * 2) as usize);
        for row in 1..=height {
            assert_eq!(
                mapping.get(&Coord::new(1, row)),
                mapping.get(&Coord::new(2, row))
            );
            assert!(gradient
                .spectrum
                .contains(mapping.get(&Coord::new(1, row)).unwrap()));
        }
    }
    let color = Color::from_hex("123456").unwrap();
    let monochrome = Gradient::with_steps(&[color.clone()], 1, false).unwrap();
    assert!(monochrome
        .build_banded_coordinate_color_mapping(1, 19, 1, 3)
        .unwrap()
        .iter()
        .all(|(_, actual)| actual == &color));
    for bounds in [(0, 19, 1, 3), (3, 2, 1, 3), (1, 19, 0, 3), (1, 19, 4, 3)] {
        assert!(gradient
            .build_banded_coordinate_color_mapping(bounds.0, bounds.1, bounds.2, bounds.3)
            .is_err());
    }
}

#[test]
fn ordinary_cli_mapping_and_insertion_order_are_unchanged() {
    let ctx = context(19);
    assert!(!ctx.final_text_bands);
    let gradient = palette();
    let canvas = &ctx.terminal.canvas;
    for direction in [
        GradientDirection::Vertical,
        GradientDirection::Horizontal,
        GradientDirection::Radial,
        GradientDirection::Diagonal,
    ] {
        let original = gradient
            .build_coordinate_color_mapping(
                canvas.text_bottom,
                canvas.text_top,
                canvas.text_left,
                canvas.text_right,
                direction,
            )
            .unwrap();
        let actual = ctx.final_gradient_mapping(&gradient, direction).unwrap();
        assert_eq!(
            actual.iter().collect::<Vec<_>>(),
            original.iter().collect::<Vec<_>>()
        );
    }
}

#[test]
fn every_embedded_effect_builds_banded_text_and_accepts_live_audio() {
    let names: Vec<String> = ttfx::cli::Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_string())
        .filter(|name| name != "matrix" && name != "rain")
        .collect();
    assert_eq!(names.len(), 35);
    for name in names {
        let cli = ttfx::cli::Cli::try_parse_from(["ttfx", &name]).unwrap();
        let mut effect = cli.effect.unwrap().build_effect();
        let mut ctx = context(19);
        ctx.final_text_bands = true;
        effect
            .build(&mut ctx)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        effect.on_audio(&mut ctx, 0.75, 0.6, true);
        assert!(effect.next_frame(&mut ctx).is_some(), "{name}: no frame");
    }
}

#[test]
fn wipe_finishes_with_exact_bands_on_actual_character_visuals() {
    let cli = ttfx::cli::Cli::try_parse_from([
        "ttfx",
        "wipe",
        "--final-gradient-stops",
        "112244",
        "44aacc",
        "ffeedd",
    ])
    .unwrap();
    let mut effect = cli.effect.unwrap().build_effect();
    let mut ctx = context(19);
    ctx.final_text_bands = true;
    effect.build(&mut ctx).unwrap();
    let mut completed = false;
    for _ in 0..10000 {
        if effect.next_frame(&mut ctx).is_none() {
            completed = true;
            break;
        }
    }
    assert!(completed, "wipe did not finish within its frame budget");
    // Wipe's authored default is 12 steps, matching palette().
    let mapping = ctx
        .final_gradient_mapping(&palette(), GradientDirection::Vertical)
        .unwrap();
    for id in &ctx.terminal.input_characters {
        let character = &ctx.terminal.arena[id.0 as usize];
        if character.input_symbol == " " {
            continue;
        }
        let fg = character
            .animation
            .current_character_visual
            .colors
            .as_ref()
            .and_then(|colors| colors.fg_color.as_ref());
        assert_eq!(
            fg,
            mapping.get(&character.input_coord),
            "{:?}",
            character.input_coord
        );
    }
}
