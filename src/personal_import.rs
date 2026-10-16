//! Import a Logos Personal Book DOCX containing [[@Bible...:book chapter:verse]]
//! milestones. The user's Bible database is separate from the installed package.
use std::{collections::BTreeMap, path::{Path, PathBuf}, process::Command};
use regex::Regex;
use rusqlite::{Connection, params};
use crate::canon::{BOOKS, Section};

pub fn personal_db_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = directories::ProjectDirs::from("", "", "leverbe-parallel")
        .ok_or("cannot find configuration directory")?;
    Ok(dir.data_dir().join("personal-bible.db"))
}

fn parse_book(s: &str) -> Option<i64> {
    let normalized = s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    BOOKS.iter().find(|b| b.aliases.iter().any(|name| *name == normalized.as_str())
        || b.name.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>() == normalized)
        .map(|b| b.nr)
}

pub fn import_docx(
    source_db: &Path, file: &Path, code: &str, name: &str, lang: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(import_docx_aligned(source_db, file, code, name, lang, None)?.verses)
}

pub fn import_docx_aligned(
    source_db: &Path, file: &Path, code: &str, name: &str, lang: &str,
    ai: Option<&crate::auto_align::AiConfig>,
) -> Result<crate::auto_align::Report, Box<dyn std::error::Error>> {
    let code = code.trim().to_uppercase();
    if code.len() < 2 || code.len() > 16 || !code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return Err("Translation code must have 2–16 ASCII letters, digits or underscores".into());
    }
    if name.trim().is_empty() { return Err("Translation name is required".into()); }
    if !file.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("docx")) {
        return Err("Choose a .docx document".into());
    }
    // The DOCX is a ZIP archive. Read only document.xml, never execute macros
    // or fetch external relationships. Limit decompressed output.
    let output = Command::new("unzip").args(["-p", "--"]).arg(file).arg("word/document.xml").output()?;
    if !output.status.success() { return Err("Unable to read word/document.xml from DOCX".into()); }
    if output.stdout.len() > 100_000_000 { return Err("DOCX XML exceeds 100 MB limit".into()); }
    let xml = String::from_utf8(output.stdout)?;
    let document = roxmltree::Document::parse(&xml)?;
    let milestone = Regex::new(r"\[\[@(?:Bible[A-Za-z0-9_]*):\s*([A-Za-z0-9 ]+?)\s+(\d+):(\d+)\]\]")?;
    let fields = Regex::new(r"(?i)\{\{field-(?:on|off):bible\}\}")?;
    let num = Regex::new(r"^\s*\d+\s+")?;
    let mut verses: BTreeMap<(i64,i64,i64),String> = BTreeMap::new();
    let mut invalid = 0usize;
    for paragraph in document.descendants().filter(|n| n.has_tag_name(("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "p"))) {
        let text = paragraph.descendants().filter(|n| n.has_tag_name(("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "t")))
            .filter_map(|n| n.text()).collect::<String>();
        let Some(cap) = milestone.captures(&text) else { continue };
        let Some(book) = parse_book(&cap[1]) else { invalid += 1; continue };
        let chapter: i64 = cap[2].parse()?;
        let verse: i64 = cap[3].parse()?;
        if !(1..=200).contains(&chapter) || !(1..=200).contains(&verse) { invalid += 1; continue; }
        let after = &text[cap.get(0).unwrap().end()..];
        // Prefer scripture between field-on and field-off, if provided.
        let selected = if let Some(start) = after.to_lowercase().find("{{field-on:bible}}") {
            let remainder = &after[start + "{{field-on:bible}}".len()..];
            remainder.split("{{field-off:bible}}").next().unwrap_or(remainder)
        } else { after };
        let selected = fields.replace_all(selected, "");
        let selected = num.replace(&selected, "");
        let normalized = selected.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() { invalid += 1; continue; }
        let key = (book, chapter, verse);
        if verses.insert(key, normalized).is_some() { return Err(format!("Duplicate verse at {book} {chapter}:{verse}").into()); }
    }
    if verses.is_empty() { return Err("No supported Logos Bible verse milestones found".into()); }
    if invalid > 0 { return Err(format!("{invalid} invalid or empty milestones; no changes were saved").into()); }

    let personal = personal_db_path()?;
    if let Some(parent) = personal.parent() { std::fs::create_dir_all(parent)?; }
    // Copy once, never modify a packaged database under /usr/share. A personal
    // copy survives package upgrades and never affects the original LeVerbe.
    if !personal.exists() { std::fs::copy(source_db, &personal)?; }
    let mut conn = Connection::open(&personal)?;
    let tx = conn.transaction()?;
    if tx.query_row("SELECT EXISTS(SELECT 1 FROM translations WHERE code=?1)", [&code], |r| r.get::<_,i64>(0))? != 0 {
        return Err(format!("Translation {code} already exists; choose a different code").into());
    }
    let sort: i64 = tx.query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM translations", [], |r| r.get(0))?;
    tx.execute("INSERT INTO translations (code,name,lang,rtl,tagged,sort) VALUES (?1,?2,?3,?4,0,?5)",
        params![code, name.trim(), lang.trim(), (lang == "he" || lang == "hbo") as i64, sort])?;
    let tid = tx.last_insert_rowid();
    let mut chapters = BTreeMap::<i64,i64>::new();
    for &(book,chapter,_) in verses.keys() {
        let entry = chapters.entry(book).or_default();
        *entry = (*entry).max(chapter);
    }
    for (book, max_ch) in chapters {
        let book_name = BOOKS.iter().find(|b| b.nr == book).map(|b| b.name).unwrap_or("Unknown");
        let section = match Section::of(book) { Section::OldTestament => 0, Section::NewTestament => 1, Section::Deuterocanon => 2 };
        tx.execute("INSERT INTO books (translation_id,nr,name,section,chapters) VALUES (?1,?2,?3,?4,?5)",
            params![tid,book,book_name,section,max_ch])?;
    }
    let mut statement = tx.prepare("INSERT INTO verses (translation_id,book,chapter,verse,text) VALUES (?1,?2,?3,?4,?5)")?;
    for ((book,chapter,verse),value) in &verses {
        statement.execute(params![tid,book,chapter,verse,value])?;
    }
    drop(statement);
    let report = crate::auto_align::align(&tx, tid, lang.trim(), ai)?;
    tx.execute_batch("INSERT INTO verses_fts(verses_fts) VALUES('rebuild');")?;
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn book_aliases() {
        assert_eq!(parse_book("gen"),Some(1));
        assert_eq!(parse_book("1Pet"),Some(60));
        assert_eq!(parse_book("Rev"),Some(66));
    }
}
