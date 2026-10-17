//! Read-only data model over `bible.db`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

#[derive(Clone, Debug)]
pub struct Translation {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub lang: String,
    pub rtl: bool,
    /// True if this translation has per-word morphology (clickable words).
    pub tagged: bool,
}

/// One word with its morphology and lexicon data, ready for the word-study UI.
#[derive(Clone, Debug, Default)]
pub struct Word {
    pub book: i64,
    pub chapter: i64,
    pub verse: i64,
    pub position: i64,
    pub text: String,       // surface form
    pub lemma: String,      // dictionary headword
    pub translit: String,
    pub strongs: String,    // "" if none
    pub morph: String,      // raw morphology code (OSHM / MorphGNT)
    pub parsing: String,
    pub gloss: String,
    pub definition: String,
}

#[derive(Clone, Debug)]
pub struct Book {
    pub nr: i64,
    pub name: String,
    pub section: i64, // 0 OT, 1 NT, 2 deuterocanon
    pub chapters: i64,
}

#[derive(Clone, Debug)]
pub struct Verse {
    pub book: i64,
    pub chapter: i64,
    pub verse: i64,
    pub text: String,
}

/// Markers around a matched word in a search snippet. Private-use code
/// points: they can never occur in the text itself — unlike «…», which is
/// how Bokmål (and French) punctuate quotations.
pub const HIT_OPEN: char = '\u{E000}';
pub const HIT_CLOSE: char = '\u{E001}';

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub book: i64,
    pub book_name: String,
    pub chapter: i64,
    pub verse: i64,
    /// Verse text with matched terms wrapped in [`HIT_OPEN`]…[`HIT_CLOSE`].
    pub snippet: String,
}

pub struct Bible {
    conn: Connection,
}

/// Shared column list for word queries (kept in sync with [`map_word`]).
const WORD_SELECT: &str = "SELECT w.book, w.chapter, w.verse, w.position, w.text,
        COALESCE(NULLIF(w.lemma,''), l.lemma, '') AS lemma,
        COALESCE(l.translit,'')   AS translit,
        COALESCE(w.strongs,'')    AS strongs,
        COALESCE(w.morph,'')      AS morph,
        COALESCE(w.parsing,'')    AS parsing,
        COALESCE(l.gloss,'')      AS gloss,
        COALESCE(l.definition,'') AS definition
     FROM words w LEFT JOIN lexicon l ON l.strongs = w.strongs";

fn map_word(r: &rusqlite::Row) -> rusqlite::Result<Word> {
    Ok(Word {
        book: r.get(0)?,
        chapter: r.get(1)?,
        verse: r.get(2)?,
        position: r.get(3)?,
        text: r.get(4)?,
        lemma: r.get(5)?,
        translit: r.get(6)?,
        strongs: r.get(7)?,
        morph: r.get(8)?,
        parsing: r.get(9)?,
        gloss: r.get(10)?,
        definition: r.get(11)?,
    })
}

/// Locate `bible.db`: `$LOGOS_DB`, then `data/` beside the manifest, then `./data`.
pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("LOGOS_DB") {
        return PathBuf::from(p);
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bible.db");
    if manifest.exists() {
        return manifest;
    }
    PathBuf::from("data/bible.db")
}

impl Bible {
    pub fn open(path: &Path) -> rusqlite::Result<Bible> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Bible { conn })
    }

    pub fn translations(&self) -> rusqlite::Result<Vec<Translation>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, code, name, lang, rtl, tagged FROM translations ORDER BY sort")?;
        let rows = stmt.query_map([], |r| {
            Ok(Translation {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
                lang: r.get(3)?,
                rtl: r.get::<_, i64>(4)? != 0,
                tagged: r.get::<_, i64>(5)? != 0,
            })
        })?;
        rows.collect()
    }

    pub fn books(&self, translation_id: i64) -> rusqlite::Result<Vec<Book>> {
        let mut stmt = self.conn.prepare(
            "SELECT nr, name, section, chapters FROM books WHERE translation_id = ?1 ORDER BY nr",
        )?;
        let rows = stmt.query_map([translation_id], |r| {
            Ok(Book {
                nr: r.get(0)?,
                name: r.get(1)?,
                section: r.get(2)?,
                chapters: r.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// Number of chapters in a book for a translation (0 if absent).
    pub fn chapter_count(&self, translation_id: i64, book: i64) -> rusqlite::Result<i64> {
        self.conn
            .query_row(
                "SELECT chapters FROM books WHERE translation_id = ?1 AND nr = ?2",
                [translation_id, book],
                |r| r.get(0),
            )
            .optional()
            .map(|o| o.unwrap_or(0))
    }

    pub fn chapter(
        &self,
        translation_id: i64,
        book: i64,
        chapter: i64,
    ) -> rusqlite::Result<Vec<Verse>> {
        let mut stmt = self.conn.prepare(
            "SELECT book, chapter, verse, text FROM verses
             WHERE translation_id = ?1 AND book = ?2 AND chapter = ?3
             ORDER BY verse",
        )?;
        let rows = stmt.query_map([translation_id, book, chapter], |r| {
            Ok(Verse {
                book: r.get(0)?,
                chapter: r.get(1)?,
                verse: r.get(2)?,
                text: r.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// Per-word morphology + lexicon data for one verse (tagged translations).
    /// Joins the Strong's lexicon so the UI has everything it needs on click.
    pub fn words(
        &self,
        translation_id: i64,
        book: i64,
        chapter: i64,
        verse: i64,
    ) -> rusqlite::Result<Vec<Word>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{WORD_SELECT} WHERE w.translation_id=?1 AND w.book=?2 AND w.chapter=?3 AND w.verse=?4 ORDER BY w.position"))?;
        let rows = stmt.query_map([translation_id, book, chapter, verse], map_word)?;
        rows.collect()
    }

    /// Original-language words carrying a given Strong's number in one verse
    /// (used to resolve an English word back to its Hebrew/Greek word).
    /// Personal import suggestions use exact in-verse source positions and
    /// remain distinct from the bundled, editorially tagged texts.
    pub fn personal_word_links(&self, tid: i64, word: &Word) -> rusqlite::Result<Option<(Vec<Word>, String)>> {
        let exists: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='personal_alignment_imports')", [], |r| r.get(0))?;
        if !exists { return Ok(None); }
        let imported: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM personal_alignment_imports WHERE translation_id=?1)",[tid], |r| r.get(0))?;
        if !imported { return Ok(None); }
        let query = format!("{WORD_SELECT} JOIN personal_word_links p ON p.source_translation_id=w.translation_id AND p.book=w.book AND p.chapter=w.chapter AND p.verse=w.verse AND p.source_position=w.position WHERE p.translation_id=?1 AND p.book=?2 AND p.chapter=?3 AND p.verse=?4 AND p.target_position=?5 ORDER BY w.position");
        let args = rusqlite::params![tid,word.book,word.chapter,word.verse,word.position];
        let mut stmt = self.conn.prepare(&query)?;
        let words: Vec<Word> = stmt.query_map(args, map_word)?.collect::<rusqlite::Result<_>>()?;
        let note = if words.is_empty() { "No reliable automatic connection found for this word.".to_string() }
            else {
                let method: String = self.conn.query_row("SELECT method FROM personal_word_links WHERE translation_id=?1 AND book=?2 AND chapter=?3 AND verse=?4 AND target_position=?5 LIMIT 1",
                    rusqlite::params![tid,word.book,word.chapter,word.verse,word.position], |r| r.get(0))?;
                match method.as_str() {
                    "prepared-reviewed"=>"Prepared connection from a context-reviewed sample. Review provenance is supplied by the document author.".into(),
                    "prepared-suggestion"=>"Unreviewed prepared suggestion. Check the verse context.".into(),
                    _=>format!("Unreviewed automatic suggestion ({}). Check the verse context.", if method == "ai-suggestion" { "AI-assisted" } else { "offline" }),
                }
            };
        let has_notes:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='personal_alignment_notes')",[],|r|r.get(0))?;
        let issue:String=if has_notes {self.conn.query_row("SELECT note FROM personal_alignment_notes WHERE translation_id=?1 AND book=?2 AND chapter=?3 AND verse=?4",rusqlite::params![tid,word.book,word.chapter,word.verse],|r|r.get(0)).unwrap_or_default()}else{String::new()};
        let note=if issue.is_empty(){note}else{format!("{note}\nSource warning: {issue}")};
        Ok(Some((words,note)))
    }

    pub fn original_occurrences(&self, strongs: &str) -> rusqlite::Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM words w JOIN translations t ON t.id=w.translation_id WHERE w.strongs=?1 AND ((t.code='WLC' AND w.book BETWEEN 1 AND 39) OR (t.code='SBLGNT' AND w.book BETWEEN 40 AND 66))",[strongs],|r|r.get(0))
    }
    pub fn surface_occurrences(&self, tid:i64, surface:&str) -> rusqlite::Result<usize> {
        let wanted=surface_key(surface);
        if wanted.is_empty() {return Ok(0);}
        let mut stmt=self.conn.prepare("SELECT text FROM verses WHERE translation_id=?1")?;
        let mut count=0;
        for text in stmt.query_map([tid],|r|r.get::<_,String>(0))? {count+=text?.split_whitespace().filter(|w|surface_key(w)==wanted).count();}
        Ok(count)
    }
    pub fn range_preview(&self,tid:i64,book:i64,chapter:i64,verse:i64,end_chapter:i64,end_verse:i64)->rusqlite::Result<String> {
        let mut stmt=self.conn.prepare("SELECT chapter,verse,text FROM verses WHERE translation_id=?1 AND book=?2 AND (chapter>?3 OR (chapter=?3 AND verse>=?4)) AND (chapter<?5 OR (chapter=?5 AND verse<=?6)) ORDER BY chapter,verse LIMIT 200")?;
        let lines=stmt.query_map(rusqlite::params![tid,book,chapter,verse,end_chapter,end_verse],|r|Ok(format!("{}:{} {}",r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(if lines.is_empty(){"This passage is not available in the leftmost translation.".into()}else{lines.join("\n")})
    }
    pub fn word_search(&self,tid:i64,strongs:&str,morph:Option<&str>,surface:Option<&str>)->rusqlite::Result<Vec<SearchHit>> {
        let mut hits=Vec::new();
        let sql=if surface.is_some() {
            "SELECT book,chapter,verse,text FROM verses WHERE translation_id=?1 ORDER BY book,chapter,verse"
        } else {
            "SELECT v.book,v.chapter,v.verse,v.text FROM verses v WHERE v.translation_id=?1 AND EXISTS(SELECT 1 FROM words w JOIN translations t ON t.id=w.translation_id WHERE w.book=v.book AND w.chapter=v.chapter AND w.verse=v.verse AND w.strongs=?2 AND (?3 IS NULL OR w.morph=?3) AND ((t.code='WLC' AND w.book BETWEEN 1 AND 39) OR (t.code='SBLGNT' AND w.book BETWEEN 40 AND 66))) ORDER BY v.book,v.chapter,v.verse"
        };
        let mut stmt=self.conn.prepare(sql)?;
        let mapper=|r:&rusqlite::Row|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?));
        let rows=if surface.is_some(){stmt.query_map([tid],mapper)?}else{stmt.query_map(rusqlite::params![tid,strongs,morph],mapper)?};
        for row in rows {
            let (book,chapter,verse,text)=row?;
            if surface.is_some_and(|s|!text.split_whitespace().any(|w|surface_key(w)==surface_key(s))) {continue;}
            hits.push(SearchHit{book,chapter,verse,book_name:crate::study::book_name(book).into(),snippet:text});
        }
        Ok(hits)
    }

    pub fn words_with_strongs(
        &self,
        translation_id: i64,
        book: i64,
        chapter: i64,
        verse: i64,
        strongs: &str,
    ) -> rusqlite::Result<Vec<Word>> {
        let mut stmt = self.conn.prepare(&format!(
            "{WORD_SELECT} WHERE w.translation_id=?1 AND w.book=?2 AND w.chapter=?3 AND w.verse=?4 AND w.strongs=?5 ORDER BY w.position"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params![translation_id, book, chapter, verse, strongs],
            map_word,
        )?;
        rows.collect()
    }

    /// The plain text of a single verse (e.g. the parallel Septuagint verse).
    pub fn verse_text(
        &self,
        translation_id: i64,
        book: i64,
        chapter: i64,
        verse: i64,
    ) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT text FROM verses WHERE translation_id=?1 AND book=?2 AND chapter=?3 AND verse=?4",
                [translation_id, book, chapter, verse],
                |r| r.get(0),
            )
            .optional()
    }

    /// Every word of a translation in canonical order, as raw rows for
    /// `search::Corpus::build`. Whole-corpus, read once and kept in memory:
    /// the advanced search evaluates against it directly.
    pub fn corpus_words(&self, translation_id: i64) -> rusqlite::Result<Vec<crate::search::RawWord>> {
        let mut stmt = self.conn.prepare(
            "SELECT w.book, w.chapter, w.verse, w.text,
                    COALESCE(NULLIF(w.lemma,''), l.lemma, ''), COALESCE(w.strongs,''), COALESCE(w.morph,'')
             FROM words w LEFT JOIN lexicon l ON l.strongs = w.strongs
             WHERE w.translation_id = ?1
             ORDER BY w.book, w.chapter, w.verse, w.position",
        )?;
        let rows = stmt.query_map([translation_id], |r| {
            Ok(crate::search::RawWord {
                book: r.get(0)?,
                chapter: r.get(1)?,
                verse: r.get(2)?,
                text: r.get(3)?,
                lemma: r.get(4)?,
                strongs: r.get(5)?,
                morph: r.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// The whole lexicon as (Strong's, headword, gloss + definition), for
    /// resolving `lemma:` and `gloss:` queries to Strong's numbers.
    pub fn lexicon_rows(&self) -> rusqlite::Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT strongs, COALESCE(lemma,''), COALESCE(gloss,'') || ' ' || COALESCE(definition,'') FROM lexicon",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// How often each lexeme (Strong's number) occurs in a whole translation.
    ///
    /// This is what a reader's edition is built on: a word is glossed in the
    /// apparatus when its lexeme is rare enough in the corpus that a reader is
    /// unlikely to know it. Counting is per-corpus, so the Hebrew Bible and the
    /// Greek New Testament each get their own scale.
    pub fn lexeme_frequencies(&self, translation_id: i64) -> rusqlite::Result<HashMap<String, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT strongs, COUNT(*) FROM words \
             WHERE translation_id = ?1 AND strongs IS NOT NULL AND strongs <> '' \
             GROUP BY strongs",
        )?;
        let rows = stmt.query_map([translation_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        rows.collect()
    }

    /// Full-text search within one translation, in canonical order — book,
    /// chapter, verse — so hits read down the page the way the Bible does.
    /// (FTS5's relevance `rank` is what made results look shuffled.)
    pub fn search(
        &self,
        translation_id: i64,
        query: &str,
        limit: i64,
    ) -> rusqlite::Result<Vec<SearchHit>> {
        let match_expr = build_match(query);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT v.book, b.name, v.chapter, v.verse,
                    snippet(verses_fts, 0, char(57344), char(57345), '…', 16) AS snip
             FROM verses_fts
             JOIN verses v ON v.id = verses_fts.rowid
             JOIN books b ON b.translation_id = v.translation_id AND b.nr = v.book
             WHERE verses_fts MATCH ?1 AND v.translation_id = ?2
             ORDER BY v.book, v.chapter, v.verse
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![match_expr, translation_id, limit], |r| {
            Ok(SearchHit {
                book: r.get(0)?,
                book_name: r.get(1)?,
                chapter: r.get(2)?,
                verse: r.get(3)?,
                snippet: r.get(4)?,
            })
        })?;
        rows.collect()
    }
}

/// Turn free-text into a safe FTS5 MATCH expression: each token becomes a
/// quoted term ANDed together, so punctuation never breaks the query syntax.
fn build_match(query: &str) -> String {
    query
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

fn surface_key(text:&str)->String {
    text.trim_matches(|c:char|!c.is_alphanumeric()).to_lowercase()
}
