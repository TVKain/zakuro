//! Integration regression tests for command-line arguments and `--debug` flag parsing.

use zakuro::cli::parse_args;
use zakuro_gpu::RendererKind;

#[test]
fn test_cli_debug_flag_parsing() {
    // Default (no debug flag)
    let opts = parse_args(vec![]).expect("failed to parse empty args");
    assert!(!opts.debug);

    // Short flag -d
    let opts = parse_args(vec!["-d".to_string()]).expect("failed to parse -d");
    assert!(opts.debug);

    // Long flag --debug
    let opts = parse_args(vec!["--debug".to_string()]).expect("failed to parse --debug");
    assert!(opts.debug);
}

#[test]
fn test_cli_renderer_and_scale() {
    let args = vec![
        "--renderer".to_string(),
        "vulkan".to_string(),
        "--scale".to_string(),
        "4".to_string(),
        "--mute".to_string(),
    ];
    let opts = parse_args(args).expect("failed to parse renderer and scale");
    assert_eq!(opts.renderer, Some(RendererKind::Vulkan));
    assert_eq!(opts.scale, Some(4));
    assert!(opts.mute);
}

#[test]
fn test_cli_rom_positional() {
    let args = vec![
        "Pokemon Ultra Sun USA.3ds".to_string(),
        "--debug".to_string(),
        "--new3ds".to_string(),
    ];
    let opts = parse_args(args).expect("failed to parse rom positional arg");
    assert_eq!(opts.rom, Some("Pokemon Ultra Sun USA.3ds".to_string()));
    assert!(opts.debug);
    assert!(opts.new3ds);
}

#[test]
fn test_cli_unknown_option_error() {
    let args = vec!["--unknown-flag".to_string()];
    let err = parse_args(args).expect_err("unknown option should return Err");
    assert!(err.contains("unknown option '--unknown-flag'"));
}
