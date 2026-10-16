//! The search engine against the real database. These pin the counts that
//! were cross-checked by hand with SQL when the engine was written; they skip
//! (rather than fail) when `data/bible.db` is not present, since the database
//! is built locally and not committed.

use std::collections::HashMap;

use logos_rs::bible::{Bible, HIT_CLOSE, HIT_OPEN};
use logos_rs::search::{self, Compiled, Corpus, Lexicon};

fn open() -> Option<(Bible, HashMap<String, i64>)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bible.db");
    if !path.exists() {
        eprintln!("skipping: {} not built", path.display());
        return None;
    }
    let bible = Bible::open(&path).expect("open database");
    let ids = bible.translations().unwrap().into_iter().map(|t| (t.code, t.id)).collect();
    Some((bible, ids))
}

fn corpus(bible: &Bible, tid: i64, hebrew: bool) -> Corpus {
    Corpus::build(bible.corpus_words(tid).unwrap(), hebrew)
}

fn count(q: &str, c: &Corpus, lex: &Lexicon) -> usize {
    Compiled::new(&search::parse(q).unwrap(), c, lex).run(c).len()
}

#[test]
fn greek_counts_match_sql() {
    let Some((bible, ids)) = open() else { return };
    let lex = Lexicon::new(bible.lexicon_rows().unwrap());
    let nt = corpus(&bible, ids["SBLGNT"], false);
    // SELECT count(DISTINCT verse) FROM words WHERE strongs='G25'  → 110
    assert_eq!(count("G25", &nt, &lex), 110);
    // … AND tense (parse position 5) = 'A'  → 29
    assert_eq!(count("[G25 aorist]", &nt, &lex), 29);
    assert_eq!(count("[lemma:ἀγαπάω aorist]", &nt, &lex), 29);
    assert_eq!(count("[lemma:αγαπαω tense:aorist]", &nt, &lex), 29); // unaccented
    // an aorist verb immediately before θεός (self-join on position+1)  → 8
    assert_eq!(count("[verb aorist] BEFORE/1 lemma:θεός", &nt, &lex), 8);
    assert_eq!(count("lemma:θεός AFTER/1 [verb aorist]", &nt, &lex), 8);
    // John 3:16 is among the aorist hits and carries the verb at its position
    let compiled = Compiled::new(&search::parse("[G25 aorist]").unwrap(), &nt, &lex);
    let hits = compiled.run(&nt);
    let john = hits.iter().find(|h| (h.verse.book, h.verse.chapter, h.verse.verse) == (43, 3, 16)).unwrap();
    assert_eq!(john.positions.len(), 1);
    assert_eq!(nt.strongs.name(nt.words[john.positions[0]].strongs), "G25");
}

#[test]
fn hebrew_counts_match_sql() {
    let Some((bible, ids)) = open() else { return };
    let lex = Lexicon::new(bible.lexicon_rows().unwrap());
    let ot = corpus(&bible, ids["WLC"], true);
    // morph LIKE 'HVqw%' OR '%/Vqw%'  → 6929 verses
    assert_eq!(count("[verb qal wayyiqtol]", &ot, &lex), 6929);
    // Genesis 1:1 opens with a qal perfect: בָּרָא
    let compiled = Compiled::new(&search::parse("[H1254 qal perfect]").unwrap(), &ot, &lex);
    let hits = compiled.run(&ot);
    assert!(hits.iter().any(|h| (h.verse.book, h.verse.chapter, h.verse.verse) == (1, 1, 1)));
    // the Hebrew lemma resolves through the lexicon (WLC has no lemma column)
    assert!(count("lemma:אלהים", &ot, &lex) > 2000);
    // a morpheme-level constraint: conjunction + verb in ONE word is not the
    // same as a conjunction anywhere and a verb anywhere
    assert!(count("[conjunction verb]", &ot, &lex) == 0);
    assert!(count("conjunction AND verb", &ot, &lex) > 10000);
}

#[test]
fn full_text_snippets_use_private_markers() {
    let Some((bible, ids)) = open() else { return };
    // Bokmål punctuates quotations with «…», which must survive as text
    let hits = bible.search(ids["NB"], "Gud", 50).unwrap();
    assert!(!hits.is_empty());
    for h in &hits {
        assert!(h.snippet.contains(HIT_OPEN) && h.snippet.contains(HIT_CLOSE), "{}", h.snippet);
    }
    let gen13 = hits.iter().find(|h| (h.book, h.chapter, h.verse) == (1, 1, 3)).expect("Genesis 1:3");
    assert!(gen13.snippet.contains('«'), "the text's own quotes are kept: {}", gen13.snippet);
    let (lines, rtl) = search::hit_lines(&gen13.snippet, HIT_OPEN, HIT_CLOSE, 200);
    assert!(!rtl);
    let hits_in_row: Vec<&str> = lines[0].iter().filter(|(_, h)| *h).map(|(w, _)| w.as_str()).collect();
    assert_eq!(hits_in_row, vec!["Gud:"]);
}

#[test]
fn frequencies_and_corpus_shape() {
    let Some((bible, ids)) = open() else { return };
    let freq = bible.lexeme_frequencies(ids["WLC"]).unwrap();
    assert_eq!(freq.len(), 8632);
    assert!(freq["H430"] > 2000); // אֱלֹהִים
    let nt = corpus(&bible, ids["SBLGNT"], false);
    assert_eq!(nt.verses.len(), 7927);
    assert!(nt.span(search::VerseId { book: 43, chapter: 3, verse: 16 }).is_some());
    assert!(nt.span(search::VerseId { book: 1, chapter: 1, verse: 1 }).is_none());
}

#[test]
fn word_study_counts_form_search_and_translation_previews() {
    let Some((bible,ids))=open() else {return};
    let kjv=ids["KJV"];let greek=ids["SBLGNT"];
    let word=bible.words_with_strongs(greek,43,3,16,"G25").unwrap().remove(0);
    let all=bible.word_search(kjv,"G25",None,None).unwrap();
    let form=bible.word_search(kjv,"G25",Some(&word.morph),None).unwrap();
    assert_eq!(all.len(),110);assert!(!form.is_empty());assert!(form.len()<all.len());
    assert!(form.iter().any(|h|(h.book,h.chapter,h.verse)==(43,3,16)));
    assert!(bible.original_occurrences("G25").unwrap()>=110);
    assert!(bible.surface_occurrences(kjv,"God,").unwrap()>2000);
    assert_eq!(bible.surface_occurrences(kjv,"God,").unwrap(),bible.surface_occurrences(kjv,"god").unwrap());
    let exact=bible.word_search(kjv,"",None,Some("loved")).unwrap();
    assert!(exact.iter().any(|h|(h.book,h.chapter,h.verse)==(43,3,16)));
    let preview=bible.range_preview(kjv,43,3,16,3,17).unwrap();
    assert!(preview.contains("3:16")&&preview.contains("3:17")&&preview.contains("For God so loved"));
    let missing=bible.range_preview(greek,1,1,1,1,1).unwrap();assert!(missing.contains("not available"));
}
