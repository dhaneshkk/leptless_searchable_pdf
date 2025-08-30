use pdfium_render::prelude::*;

use std::fs::File;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::Command;

use image::{ColorType, ExtendedColorType, ImageEncoder};
use image::codecs::png::PngEncoder;

use tempfile::TempDir;
use anyhow::{anyhow, Context, Result};
use clap::Parser;

/// Simple PDF OCR tool
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Input PDF file
    #[arg(short, long)]
    input: PathBuf,

    /// Output searchable PDF file
    #[arg(short, long, default_value = "ocr_output.pdf")]
    output: PathBuf,

    /// OCR language (Tesseract)
    #[arg(short, long, default_value = "eng")]
    lang: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.input.exists() {
        return Err(anyhow!("Input file '{}' does not exist.", args.input.display()));
    }

    // Temporary directory for intermediate PNGs
    let temp_dir: TempDir = tempfile::tempdir().context("Failed to create temporary directory")?;
    let temp_dir_path = temp_dir.path();

    // ---- Init PDFium ----
    let bindings = Pdfium::bind_to_library(
        Pdfium::pdfium_platform_library_name_at_path("./lib"),
    ).or_else(|_| Pdfium::bind_to_system_library())?;
    let pdfium = Pdfium::new(bindings);

    // ---- Load PDF ----
    let doc = pdfium.load_pdf_from_file(&args.input, None)?;
    let page_count = doc.pages().len();
    println!("Loaded PDF with {} pages", page_count);

    // ---- Step 1: Render each page to PNG file ----
    let mut image_paths = Vec::new();

    for (index, page) in doc.pages().iter().enumerate() {
        println!("Rendering page {}...", index + 1);

        // Render page at ~300 DPI
        let rendered = page.render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(2480)
                .set_target_height(3508),
        )?;
        let rgb_image = rendered.as_image().to_rgb8();

        // Encode to PNG on disk
        let image_path = temp_dir_path.join(format!("page_{:04}.png", index + 1));
        let mut png_file = File::create(&image_path)?;
        {
            // Create a Vec that lives long enough
            let mut png_data: Vec<u8> = Vec::new();

            // Pass it to the Cursor
            let mut cursor = Cursor::new(&mut png_data);
            let encoder = PngEncoder::new(&mut cursor);
            encoder.write_image(
                &rgb_image,
                rgb_image.width(),
                rgb_image.height(),
                ExtendedColorType::from(ColorType::Rgb8),
            )?;
            // Save to file
            png_file.write_all(&cursor.into_inner())?;
        }

        image_paths.push(image_path);
    }

    // ---- Step 2: Generate searchable PDF with Tesseract ----
    println!("Running Tesseract to create searchable PDF...");

    // Tesseract expects a "file list" or individual files
    // We'll write a temporary file list
    let file_list_path = temp_dir_path.join("images.txt");
    let mut file_list = File::create(&file_list_path)?;
    for img in &image_paths {
        writeln!(file_list, "{}", img.display())?;
    }

    // Tesseract command: tesseract file_list output.pdf -l lang pdf
    let mut cmd = Command::new("tesseract");
    cmd.arg(file_list_path);
    cmd.arg(&args.output);
    cmd.args(&["-l", &args.lang, "pdf"]);
    let status = cmd.status().context("Failed to run tesseract")?;
    if !status.success() {
        return Err(anyhow!("Tesseract OCR failed"));
    }

    println!("\n✅ Searchable PDF generated at '{}'", args.output.display());
    Ok(())
}
