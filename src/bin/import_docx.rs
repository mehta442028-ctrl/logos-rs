use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 && !(args.len() == 6 && args[5] == "--ai") {
        eprintln!("Usage: import-docx <file.docx> <CODE> <Translation name> <language> [--ai]");
        std::process::exit(2);
    }
    let ai = if args.len() == 6 {
        let model = std::env::var("LEVERBE_ALIGNMENT_MODEL").unwrap_or_default();
        let limit = std::env::var("LEVERBE_AI_MAX_VERSES").unwrap_or_else(|_| "20".into()).parse()?;
        Some(logos_rs::auto_align::AiConfig::from_env(&model, limit)?)
    } else { None };
    let base = logos_rs::importer::db_path();
    if !base.exists() { return Err("Install or build the bundled Bible database first".into()); }
    let report = logos_rs::personal_import::import_docx_aligned(&base, Path::new(&args[1]), &args[2], &args[3], &args[4], ai.as_ref())?;
    println!("{report} Restart LeVerbe Parallel to select the translation.");
    Ok(())
}
