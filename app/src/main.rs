//! PLUME desktop entry point.
//!
//! `plume` opens the editor. `plume --headless` renders a gallery scene or a
//! document to an image without opening a window, which is what CI and the
//! documentation screenshots use.
//!
//! All file operations are constrained to the PLUME sandbox (the workspace
//! parent by default, or `PLUME_SANDBOX`). No installer, registry key or
//! service is touched.

mod app;
mod gpu;
mod sandbox;
mod theme;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use plume_core::gallery;
use plume_core::scene::MeshQuality;

use crate::app::PlumeApp;
use crate::sandbox::Sandbox;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,plume_core=info"),
    )
    .init();
    if let Err(error) = run() {
        eprintln!("plume: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse(std::env::args().skip(1))?;
    if args.help {
        print_help();
        return Ok(());
    }
    if args.version {
        println!("PLUME {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.headless {
        return render_headless(&args);
    }
    run_gui(&args)
}

fn render_headless(args: &Args) -> Result<()> {
    let sandbox = Sandbox::discover()?;
    if let Some(id) = &args.gallery {
        validate_gallery(id)?;
    }
    let mut doc = match (&args.gallery, &args.file) {
        (Some(id), _) => {
            gallery::build_document(id).with_context(|| format!("unknown gallery scene {id}"))?
        }
        (None, Some(path)) => {
            let path = PathBuf::from(path);
            let (doc, notes, _) = PlumeApp::load_document_static(&sandbox, &path)
                .with_context(|| format!("could not open {}", path.display()))?;
            for note in notes {
                eprintln!("plume: {note}");
            }
            doc
        }
        (None, None) => gallery::build_document("ribbon-bloom")
            .context("the built-in ribbon-bloom gallery scene is missing")?,
    };
    doc.scene.touch();
    let width = args.width;
    let height = args.height;
    let samples = args.samples.max(1);
    let destination = args
        .output
        .clone()
        .or_else(|| args.render.clone())
        .unwrap_or_else(|| sandbox.root().join("render.png"));
    let destination = sandbox
        .ensure_file(&destination)
        .with_context(|| format!("invalid render destination {}", destination.display()))?;
    let format = render_format(&destination)?;
    eprintln!(
        "plume: rendering {} at {width}x{height} ({}, {} {})",
        doc.name,
        args.quality.label(),
        samples,
        if samples == 1 { "sample" } else { "samples" }
    );
    let image = gpu::render_document_headless(&doc, width, height, args.quality, args.samples)
        .context("headless GPU render failed")?;
    let (image_width, image_height) = (image.width(), image.height());
    match format {
        image::ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image)
            .to_rgb8()
            .save_with_format(&destination, format),
        _ => image.save_with_format(&destination, format),
    }
    .with_context(|| format!("could not write {}", destination.display()))?;
    eprintln!(
        "plume: wrote {} ({}x{})",
        destination.display(),
        image_width,
        image_height
    );
    Ok(())
}

fn render_format(path: &Path) -> Result<image::ImageFormat> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => Ok(image::ImageFormat::Png),
        Some("jpg" | "jpeg") => Ok(image::ImageFormat::Jpeg),
        Some("gif") => Ok(image::ImageFormat::Gif),
        Some(other) => bail!("unsupported render image extension .{other}; use .png, .jpg or .gif"),
        None => bail!("render destination has no image extension; use .png, .jpg or .gif"),
    }
}

fn run_gui(args: &Args) -> Result<()> {
    let startup = args.file.as_ref().map(PathBuf::from);
    if let Some(id) = &args.gallery {
        validate_gallery(id)?;
    }

    let viewport = egui::ViewportBuilder::default()
        .with_title("PLUME")
        .with_inner_size([1280.0, 680.0])
        .with_min_inner_size([1080.0, 620.0])
        .with_app_id("plume");

    let wgpu_options = if software_requested() {
        software_wgpu_configuration()
    } else {
        egui_wgpu::WgpuConfiguration::default()
    };

    let options = eframe::NativeOptions {
        viewport,
        wgpu_options,
        persist_window: true,
        ..Default::default()
    };
    let gallery_start = args.gallery.clone();
    eframe::run_native(
        "PLUME",
        options,
        Box::new(move |cc| {
            let mut app = PlumeApp::new(cc, startup.clone()).map_err(
                |error| -> Box<dyn std::error::Error + Send + Sync> { format!("{error:#}").into() },
            )?;
            if let Some(id) = &gallery_start {
                app.load_gallery(id);
            }
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|error| anyhow::anyhow!("PLUME window failed: {error}"))?;
    Ok(())
}

fn software_requested() -> bool {
    match std::env::var("PLUME_SOFTWARE") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "off" | "no"
        ),
        Err(_) => false,
    }
}

/// Select a software adapter for hosts whose Vulkan/DX12 driver crashes on
/// device creation. The same WARP/llvmpipe path backs headless capture.
fn software_wgpu_configuration() -> egui_wgpu::WgpuConfiguration {
    let mut configuration = egui_wgpu::WgpuConfiguration::default();
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.native_adapter_selector = Some(Arc::new(|adapters, _surface| {
        adapters
            .iter()
            .find(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu)
            .or_else(|| {
                adapters.iter().find(|adapter| {
                    matches!(
                        adapter.get_info().backend,
                        wgpu::Backend::Gl | wgpu::Backend::BrowserWebGpu
                    )
                })
            })
            .or_else(|| adapters.first())
            .cloned()
            .ok_or_else(|| "no suitable software adapter was found".to_string())
    }));
    configuration.wgpu_setup = egui_wgpu::WgpuSetup::CreateNew(setup);
    configuration
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Args {
    file: Option<String>,
    gallery: Option<String>,
    headless: bool,
    render: Option<PathBuf>,
    output: Option<PathBuf>,
    width: u32,
    height: u32,
    quality: MeshQuality,
    samples: u32,
    help: bool,
    version: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            file: None,
            gallery: None,
            headless: false,
            render: None,
            output: None,
            width: 1600,
            height: 1000,
            quality: MeshQuality::Normal,
            samples: 1,
            help: false,
            version: false,
        }
    }
}

impl Args {
    fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self> {
        let mut args = Args::default();
        let mut positional: Vec<String> = Vec::new();
        while let Some(argument) = arguments.next() {
            let mut value = |name: &str| -> Result<String> {
                arguments
                    .next()
                    .with_context(|| format!("{name} needs a value"))
            };
            match argument.as_str() {
                "-h" | "--help" => args.help = true,
                "-V" | "--version" => args.version = true,
                "--headless" => args.headless = true,
                "--render" => {
                    args.headless = true;
                    args.render = Some(PathBuf::from(value("--render")?));
                }
                "--output" | "-o" => {
                    args.headless = true;
                    args.output = Some(PathBuf::from(value("--output")?));
                }
                "--gallery" => args.gallery = Some(value("--gallery")?),
                "--file" => args.file = Some(value("--file")?),
                "--width" => {
                    args.width = value("--width")?
                        .parse()
                        .context("--width must be a number")?
                }
                "--height" => {
                    args.height = value("--height")?
                        .parse()
                        .context("--height must be a number")?
                }
                "--samples" => {
                    args.samples = value("--samples")?
                        .parse()
                        .context("--samples must be a number")?
                }
                "--quality" => {
                    let raw = value("--quality")?;
                    args.quality = match raw.to_ascii_lowercase().as_str() {
                        "draft" => MeshQuality::Draft,
                        "normal" => MeshQuality::Normal,
                        "high" => MeshQuality::High,
                        "export" => MeshQuality::Export,
                        other => {
                            bail!("unknown quality {other}; use draft, normal, high or export")
                        }
                    };
                }
                "--" => positional.extend(arguments.by_ref()),
                other if other.starts_with('-') => {
                    bail!("unknown option {other}; run plume --help")
                }
                other => positional.push(other.to_string()),
            }
        }

        if let Some(first) = positional.first() {
            if Path::new(first).extension().is_some() || Path::new(first).exists() {
                args.file = Some(first.clone());
            } else if gallery::build_document(first).is_some() {
                args.gallery = Some(first.clone());
            } else {
                bail!(
                    "{first} is neither a scene file nor a gallery scene.\n\
                     Available gallery scenes:\n{}",
                    gallery_cli_list()
                );
            }
        }
        if positional.len() > 1 {
            bail!(
                "too many positional arguments: {}",
                positional[1..].join(" ")
            );
        }
        if args.gallery.is_some() && args.file.is_some() {
            bail!("--gallery and --file cannot be combined; choose one scene");
        }
        if args.output.is_none() {
            args.output = args.render.clone();
        }
        if !args.help && !args.version {
            if !(16..=7680).contains(&args.width) {
                bail!(
                    "--width must be between 16 and 7680 pixels (got {})",
                    args.width
                );
            }
            if !(16..=7680).contains(&args.height) {
                bail!(
                    "--height must be between 16 and 7680 pixels (got {})",
                    args.height
                );
            }
        }
        Ok(args)
    }
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> String {
    format!(
        r#"PLUME {version} — a 3D curve studio

Usage:
  plume [FILE|GALLERY] [options]

Opening the editor:
  plume                          Start with the ribbon-bloom gallery scene
  plume path/to/scene.plume      Open a PLUME or Feather document, OBJ, glTF or STL
  plume ribbon-bloom             Start from a named gallery scene

Gallery scenes:
{galleries}

Headless capture:
  plume --headless --output render.png
  plume --render render.png --width 2400 --height 1500 --quality export
  plume --gallery ribbon-bloom --render ribbon.png
  plume scene.plume --render scene.png

Options:
  --gallery <id>      Gallery scene to open or render
  --file <path>       Scene or mesh to open (alternative to a positional path)
  --headless          Render without a window
  --render <path>     Headless output image (implies --headless)
  -o, --output <path> Output image (alias for --render; implies --headless)
  --width <px>        Render width, 16..=7680 (default 1600)
  --height <px>       Render height, 16..=7680 (default 1000)
  --quality <level>   draft, normal, high or export (default normal)
  --samples <n>       MSAA hint; the current post stack is single-sample
  -h, --help          Show this help
  -V, --version       Show the version

Environment:
  PLUME_SANDBOX=path  Move the file-operation sandbox (default: workspace parent)
  PLUME_SOFTWARE=1    Select a software wgpu adapter in the editor
  PLUME_HEADLESS_GPU=1 Use the fastest adapter for headless capture

Every read and write stays inside the sandbox. Nothing is installed and
no system settings are changed."#,
        version = env!("CARGO_PKG_VERSION"),
        galleries = gallery_cli_list()
    )
}

fn gallery_cli_list() -> String {
    gallery::GALLERY
        .iter()
        .map(|item| format!("  {:<18} {} — {}", item.id, item.name, item.description))
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate_gallery(id: &str) -> Result<()> {
    if gallery::build_document(id).is_some() {
        return Ok(());
    }
    bail!(
        "unknown gallery scene: {id}\n\nAvailable gallery scenes:\n{}",
        gallery_cli_list()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Args> {
        Args::parse(arguments.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn rejects_zero_render_dimensions() {
        let error = parse(&["--headless", "--width", "0"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("--width must be between 16 and 7680"),
            "{error}"
        );

        let error = parse(&["--headless", "--height", "0"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("--height must be between 16 and 7680"),
            "{error}"
        );
    }

    #[test]
    fn rejects_oversized_render_dimensions() {
        let error = parse(&["--headless", "--width", "7681"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("--width must be between 16 and 7680"),
            "{error}"
        );
    }

    #[test]
    fn accepts_render_dimension_bounds() {
        let args = parse(&["--headless", "--width", "16", "--height", "7680"]).unwrap();
        assert_eq!(args.width, 16);
        assert_eq!(args.height, 7680);
    }

    #[test]
    fn help_wins_over_invalid_dimensions() {
        let args = parse(&["--help", "--width", "0"]).unwrap();
        assert!(args.help);
    }

    #[test]
    fn help_text_keeps_its_indentation() {
        let help = help_text();
        assert!(help.contains("\n  --width <px>"));
        assert!(help.contains("\n  ribbon-bloom"));
        assert!(help.contains("16..=7680"));
    }

    #[test]
    fn unknown_gallery_lists_the_available_scenes() {
        let error = validate_gallery("not-a-scene").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("unknown gallery scene: not-a-scene"));
        assert!(message.contains("ribbon-bloom"));
        assert!(message.contains("clean-stage"));
        validate_gallery("ribbon-bloom").unwrap();
    }

    #[test]
    fn output_option_implies_headless_capture() {
        let args = parse(&["--output", "render.png"]).unwrap();
        assert!(args.headless);
        assert_eq!(args.output, Some(PathBuf::from("render.png")));
        assert_eq!(args.render, None);

        let args = parse(&["-o", "render.png"]).unwrap();
        assert!(args.headless);
        assert_eq!(args.output, Some(PathBuf::from("render.png")));
    }

    #[test]
    fn gallery_and_file_are_mutually_exclusive() {
        let error = parse(&[
            "--gallery",
            "ribbon-bloom",
            "--file",
            "testdata/import-cube.obj",
        ])
        .unwrap_err();
        assert!(error.to_string().contains("cannot be combined"), "{error}");

        let error = parse(&["--gallery", "ribbon-bloom", "testdata/import-cube.obj"]).unwrap_err();
        assert!(error.to_string().contains("cannot be combined"), "{error}");
    }

    #[test]
    fn render_formats_are_named_explicitly() {
        assert_eq!(
            render_format(Path::new("capture.png")).unwrap(),
            image::ImageFormat::Png
        );
        assert_eq!(
            render_format(Path::new("capture.JPG")).unwrap(),
            image::ImageFormat::Jpeg
        );
        assert_eq!(
            render_format(Path::new("capture.jpeg")).unwrap(),
            image::ImageFormat::Jpeg
        );
        assert_eq!(
            render_format(Path::new("capture.gif")).unwrap(),
            image::ImageFormat::Gif
        );

        let error = render_format(Path::new("capture.xyz")).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported render image extension .xyz"),
            "{error}"
        );
        let error = render_format(Path::new("capture")).unwrap_err();
        assert!(
            error.to_string().contains("has no image extension"),
            "{error}"
        );
    }
}
