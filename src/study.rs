//! Offline study resources, verse links and commentary-library access.
use std::path::PathBuf;
use rusqlite::Connection;
use crate::canon::BOOKS;

pub const BLB_BOOKS: &[&str] = &["gen","exo","lev","num","deu","jos","jdg","rth","1sa","2sa","1ki","2ki","1ch","2ch","ezr","neh","est","job","psa","pro","ecc","sng","isa","jer","lam","eze","dan","hos","joe","amo","oba","jon","mic","nah","hab","zep","hag","zec","mal","mat","mar","luk","jhn","act","rom","1co","2co","gal","eph","phl","col","1th","2th","1ti","2ti","tit","phm","heb","jas","1pe","2pe","1jo","2jo","3jo","jde","rev"];
pub fn book_name(book: i64) -> &'static str { BOOKS.iter().find(|b|b.nr==book).map(|b|b.name).unwrap_or("Unknown") }
pub fn verse_label(book: i64, chapter: i64, verse: i64) -> String { format!("{} {chapter}:{verse}",book_name(book)) }
fn encode(s: &str) -> String { s.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-._~".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect() }
pub fn verse_url(book: i64, chapter: i64, verse: i64, site: i32) -> Option<String> {
    if !(1..=66).contains(&book) || chapter < 1 || verse < 1 { return None; }
    Some(match site {
        0 | 1 => format!("https://www.biblegateway.com/verse/{}/{}",if site==0 {"en"}else{"no"},encode(&verse_label(book,chapter,verse))),
        2 => format!("https://www.blueletterbible.org/kjv/{}/{chapter}/{verse}/",BLB_BOOKS[(book-1) as usize]),
        _ => return None,
    })
}
pub fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os="linux")]
    let child = std::process::Command::new("xdg-open").arg(url).spawn()?;
    #[cfg(target_os="macos")]
    let child = std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os="windows")]
    let child = std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(url).spawn()?;
    let _ = child;
    Ok(())
}
#[derive(Clone,Debug)]
pub struct CrossReference { pub phrase: String, pub label: String, pub book:i64, pub chapter:i64, pub verse:i64, pub end_chapter:i64, pub end_verse:i64 }
#[derive(Clone,Debug)]
pub struct Work { pub id:i64, pub title:String, pub source:String, pub notice:String }
#[derive(Clone,Debug)]
pub struct Section { pub id:i64, pub title:String }
pub struct Library { conn:Connection }
pub fn library_path() -> PathBuf {
    if let Ok(path) = std::env::var("LEVERBE_LIBRARY") { return path.into(); }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for p in [dir.join("../share/leverbe-parallel/study-library.db"),dir.join("data/study-library.db")] {
                if p.is_file() { return p; }
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/study-library.db")
}
impl Library {
    pub fn open() -> rusqlite::Result<Self> { Ok(Self { conn:Connection::open_with_flags(library_path(),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)? }) }
    pub fn works(&self) -> rusqlite::Result<Vec<Work>> {
        self.conn.prepare("SELECT id,title,source,notice FROM works ORDER BY sort,id")?.query_map([],|r|Ok(Work{id:r.get(0)?,title:r.get(1)?,source:r.get(2)?,notice:r.get(3)?}))?.collect()
    }
    pub fn sections(&self,work:i64,query:&str) -> rusqlite::Result<Vec<Section>> {
        self.conn.prepare("SELECT id,title FROM sections WHERE work_id=?1 AND (?2='' OR instr(lower(title),lower(?2))>0 OR instr(lower(body),lower(?2))>0) ORDER BY ordinal")?
            .query_map(rusqlite::params![work,query],|r|Ok(Section{id:r.get(0)?,title:r.get(1)?}))?.collect()
    }
    pub fn paragraphs(&self,id:i64) -> rusqlite::Result<Vec<String>> {
        let body:String=self.conn.query_row("SELECT body FROM sections WHERE id=?1",[id],|r|r.get(0))?;
        Ok(body.split("\n\n").map(str::trim).filter(|s|!s.is_empty()).map(str::to_string).collect())
    }
    pub fn tsk(&self,book:i64,chapter:i64,verse:i64)->rusqlite::Result<(String,Vec<CrossReference>)> {
        let text=self.conn.query_row("SELECT body FROM tsk_entries WHERE book=?1 AND chapter=?2 AND verse=?3",[book,chapter,verse],|r|r.get(0)).unwrap_or_default();
        let refs=self.conn.prepare("SELECT phrase,label,target_book,target_chapter,target_verse,end_chapter,end_verse FROM tsk_refs WHERE book=?1 AND chapter=?2 AND verse=?3 ORDER BY ordinal")?
            .query_map([book,chapter,verse],|r|Ok(CrossReference{phrase:r.get(0)?,label:r.get(1)?,book:r.get(2)?,chapter:r.get(3)?,verse:r.get(4)?,end_chapter:r.get(5)?,end_verse:r.get(6)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((text,refs))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn links_keep_verse_and_language() {
        assert_eq!(verse_url(43,3,16,0).unwrap(),"https://www.biblegateway.com/verse/en/John%203%3A16");
        assert!(verse_url(43,3,16,1).unwrap().contains("/verse/no/"));
        assert_eq!(verse_url(43,3,16,2).unwrap(),"https://www.blueletterbible.org/kjv/jhn/3/16/");
        assert!(verse_url(100,1,1,0).is_none());
    }
}
