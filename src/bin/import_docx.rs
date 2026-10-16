use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("Usage: import-docx <file.docx> <CODE> <Translation name> <language (nb/en/grc/hbo)>");
        std::process::exit(2);
    }
    let base = logos_rs::importer::db_path();
    if !base.exists() { return Err("Install or build the bundled Bible database first".into()); }
    let count = logos_rs::personal_import::import_docx(&base, Path::new(&args[1]), &args[2], &args[3], &args[4])?;
    println!("Imported {count} verses. Restart LeVerbe Parallel to select the translation.");
    Ok(())
}
