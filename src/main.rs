// No console window behind the app in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use logos_rs::bible::{self, Bible, Book, HIT_CLOSE, HIT_OPEN, SearchHit, Translation, Word};
use logos_rs::canon::Section;
use logos_rs::reference;
use logos_rs::search::{self, Corpus, Lexicon, VerseId};
use logos_rs::session::{self, Session};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

slint::include_modules!();

/// Mutable application state shared across UI callbacks (single-threaded).
struct State {
    bible: Bible,
    translations: Vec<Translation>,
    visible: Vec<usize>,
    hidden: HashSet<String>,
    primary: usize,
    secondary: usize,
    extra: Vec<usize>,
    extra_words: Vec<Vec<Word>>,
    primary_books: Vec<Book>,
    book: i64,
    chapter: i64,
    last_results: Vec<SearchHit>,
    /// Flat per-chapter word lists for the two columns, indexed by chip idx.
    primary_words: Vec<Word>,
    secondary_words: Vec<Word>,
    /// Corpus frequency of every lexeme in `freq_tid`, for the reader's
    /// edition. Built once per translation — it is a whole-corpus count.
    freq: HashMap<String, i64>,
    freq_tid: i64,
    threshold_idx: usize,
    /// Set once the reader picks a cut-off by hand, after which the per-corpus
    /// default stops overriding it.
    threshold_pinned: bool,
    /// Language the current cut-off was chosen for.
    threshold_lang: String,
    /// Verse to scroll to on the next refresh (a search hit); consumed by it,
    /// so every other navigation lands at the top of the chapter.
    target_verse: Option<i64>,
    /// Advanced search runs in memory over whole translations; each is read
    /// from the database once, on first use, and kept.
    corpora: HashMap<i64, Rc<Corpus>>,
    lexicon: Option<Rc<Lexicon>>,
    /// Background loads in flight (see `spawn_preload`).
    loading: Vec<std::sync::mpsc::Receiver<Preloaded>>,
}

/// Frequency cut-offs offered for the reader's apparatus. A word is glossed
/// when its lexeme occurs fewer than this many times in the corpus.
const THRESHOLDS: [i64; 6] = [10, 25, 30, 50, 100, 200];

/// A reader's edition is an original-language text: the apparatus glosses the
/// Hebrew or Greek a reader is working through. The Strong's-tagged modern
/// translations carry no morphology of their own to parse.
fn is_original_language(lang: &str) -> bool {
    lang == "hbo" || lang == "grc"
}

/// The cut-off printed reader's editions settle on for each corpus: fewer than
/// 100× for the Hebrew Bible (as in Zondervan's reader's Hebrew Bible), fewer
/// than 30× for the Greek New Testament (as in the UBS reader's edition).
fn default_threshold(lang: &str) -> usize {
    match lang {
        "hbo" => 4, // 100
        "grc" => 2, // 30
        _ => 3,     // 50
    }
}

/// Proper names carry nothing a reader needs looked up — every translation
/// simply transliterates them — so printed reader's editions leave them out of
/// the apparatus. The Hebrew morphology tags them (`Np` opening any segment of
/// the OSHM code); MorphGNT has no such tag, but the Greek text capitalises the
/// lemma of a name and nothing else.
fn is_proper_noun(w: &Word) -> bool {
    if w.lemma.chars().next().is_some_and(char::is_uppercase) {
        return true;
    }
    // The language letter (H Hebrew / A Aramaic) prefixes only the first
    // segment, so strip it before testing. Guards against `VNp…` — a Niphal
    // perfect verb, which merely contains "Np".
    w.morph
        .split('/')
        .any(|seg| seg.strip_prefix(['H', 'A']).unwrap_or(seg).starts_with("Np"))
}

/// What the apparatus needs while a chapter is laid out.
struct Reader<'a> {
    freq: &'a HashMap<String, i64>,
    max_freq: i64,
}

impl Reader<'_> {
    /// Worth glossing: rare enough in the corpus, known to the lexicon, and
    /// not a name.
    fn glosses(&self, w: &Word) -> bool {
        !w.strongs.is_empty()
            && self.freq.get(&w.strongs).is_some_and(|&n| n < self.max_freq)
            && !is_proper_noun(w)
    }

    fn count(&self, strongs: &str) -> i64 {
        self.freq.get(strongs).copied().unwrap_or(0)
    }
}

impl State {
    /// The in-memory corpus of a translation, loaded on first use.
    fn corpus(&mut self, tid: i64) -> Rc<Corpus> {
        if let Some(c) = self.corpora.get(&tid) {
            return c.clone();
        }
        let hebrew = self.translations.iter().any(|t| t.id == tid && t.lang == "hbo");
        let rows = self.bible.corpus_words(tid).unwrap_or_default();
        let c = Rc::new(Corpus::build(rows, hebrew));
        self.corpora.insert(tid, c.clone());
        c
    }

    fn lexicon(&mut self) -> Rc<Lexicon> {
        if let Some(l) = &self.lexicon {
            return l.clone();
        }
        let l = Rc::new(Lexicon::new(self.bible.lexicon_rows().unwrap_or_default()));
        self.lexicon = Some(l.clone());
        l
    }

    fn translation_by_code(&self, code: &str) -> Option<&Translation> {
        self.translations.iter().find(|t| t.code == code)
    }

    fn primary_id(&self) -> i64 {
        self.translations[self.primary].id
    }
    fn secondary_id(&self) -> i64 {
        self.translations[self.secondary].id
    }
    fn book_name(&self, nr: i64) -> String {
        self.primary_books
            .iter()
            .find(|b| b.nr == nr)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| format!("Book {nr}"))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Locate the database; build it from the vendored sources on first run.
    let db = match std::env::var("LOGOS_DB") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => logos_rs::importer::db_path(),
    };
    if !db.exists() {
        eprintln!("Building Bible database (one-time, ~10s, offline from bundled sources)…");
        logos_rs::importer::run()?;
    }
    // A user-imported translation is stored in an isolated writable copy.
    // Prefer it on subsequent launches; bundled files remain unmodified.
    let personal_db = logos_rs::personal_import::personal_db_path()?;
    let db = if personal_db.is_file() { personal_db } else { db };
    let bible = Bible::open(&db)?;
    let translations = bible.translations()?;
    if translations.is_empty() {
        eprintln!("no translations in database");
        std::process::exit(1);
    }
    // Pick up where the last run left off. Every field is a hint: anything
    // that no longer resolves falls back to the default.
    let saved = session::load();
    let primary = translations.iter().position(|t| t.code == saved.primary).unwrap_or(0);
    // Default secondary = a different translation if available (LSG).
    let secondary = translations
        .iter()
        .position(|t| t.code == saved.secondary)
        .unwrap_or(if translations.len() > 1 { 1 } else { 0 });
    let extra: Vec<usize> = saved.extra_translations.iter().take(2)
        .filter_map(|code| translations.iter().position(|t| &t.code == code)).collect();
    let primary_books = bible.books(translations[primary].id)?;
    // The saved book may not exist in this translation (the NT-only texts, a
    // rebuilt database); fall back to its first book, and to its opening
    // chapter — a chapter number carried over from a different book is
    // meaningless.
    let (book, chapter) = match primary_books.iter().any(|b| b.nr == saved.book) {
        true => (saved.book, saved.chapter.max(1)),
        false => (primary_books.first().map_or(1, |b| b.nr), 1),
    };
    let start_lang = translations[primary].lang.clone();
    // A cut-off is remembered only when it was chosen by hand, and by value —
    // so it survives the list of offered cut-offs changing.
    let pinned = saved.threshold.and_then(|v| THRESHOLDS.iter().position(|&t| t == v));
    let threshold_idx = pinned.unwrap_or_else(|| default_threshold(&start_lang));

    let hidden: HashSet<String> = saved.hidden_translations.iter().cloned().collect();
    let visible: Vec<usize> = (0..translations.len()).filter(|&i|
        !hidden.contains(&translations[i].code) || i == primary || i == secondary || extra.contains(&i)
    ).collect();
    let state = Rc::new(RefCell::new(State {
        bible,
        translations,
        visible,
        hidden,
        primary,
        secondary,
        extra,
        extra_words: vec![Vec::new(), Vec::new()],
        primary_books,
        book,
        chapter,
        last_results: Vec::new(),
        primary_words: Vec::new(),
        secondary_words: Vec::new(),
        freq: HashMap::new(),
        freq_tid: 0,
        threshold_idx,
        threshold_pinned: pinned.is_some(),
        threshold_lang: start_lang,
        target_verse: None,
        corpora: HashMap::new(),
        lexicon: None,
        loading: Vec::new(),
    }));

    let ui = MainWindow::new()?;
    restore(&ui, &saved, primary, secondary);
    ui.set_extra_count(state.borrow().extra.len() as i32);
    update_extra_indices(&ui, &state.borrow());

    // Warm the advanced search: load the Hebrew, the Greek and the current
    // translation in the background so the first query is instant.
    {
        let mut st = state.borrow_mut();
        let jobs = preload_jobs(&st);
        let rx = spawn_preload(db.clone(), jobs, true);
        st.loading.push(rx);
    }
    let poll = slint::Timer::default();
    {
        let state = state.clone();
        poll.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
            let mut st = state.borrow_mut();
            let mut got = Vec::new();
            let mut done = Vec::new();
            for (i, rx) in st.loading.iter().enumerate() {
                loop {
                    match rx.try_recv() {
                        Ok(p) => got.push(p),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            done.push(i);
                            break;
                        }
                    }
                }
            }
            for i in done.into_iter().rev() {
                st.loading.remove(i);
            }
            for p in got {
                match p {
                    Preloaded::Corpus(tid, c) => {
                        st.corpora.entry(tid).or_insert_with(|| Rc::new(*c));
                    }
                    Preloaded::Lexicon(l) => {
                        if st.lexicon.is_none() {
                            st.lexicon = Some(Rc::new(l));
                        }
                    }
                }
            }
        });
    }

    // Static: translation labels for the combo boxes.
    {
        let st = state.borrow();
        refresh_translation_menu(&ui, &st);

        let thresholds: Vec<SharedString> =
            THRESHOLDS.iter().map(|n| format!("fewer than {n}×").into()).collect();
        ui.set_threshold_labels(ModelRc::new(VecModel::from(thresholds)));
        ui.set_threshold_index(st.threshold_idx as i32);
    }

    refresh_books(&ui, &state.borrow());
    refresh_reading(&ui, &state);

    // ---- callbacks ----
    // Each callback captures a weak UI handle and a clone of the shared state.
    macro_rules! ctx {
        () => {{
            (ui.as_weak(), state.clone())
        }};
    }

    {
        let (w, state) = ctx!();
        ui.on_select_book(move |nr| {
            let ui = w.unwrap();
            {
                let mut st = state.borrow_mut();
                st.book = nr as i64;
                st.chapter = 1;
            }
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_select_chapter(move |ch| {
            let ui = w.unwrap();
            state.borrow_mut().chapter = ch as i64;
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_prev_chapter(move || {
            let ui = w.unwrap();
            step_chapter(&state, -1);
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_next_chapter(move || {
            let ui = w.unwrap();
            step_chapter(&state, 1);
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        let db_for_preload = db.clone();
        ui.on_primary_changed(move |idx| {
            let ui = w.unwrap();
            {
                let mut st = state.borrow_mut();
                let Some(&selected) = st.visible.get(idx as usize) else { return };
                st.primary = selected;
                let id = st.primary_id();
                st.primary_books = st.bible.books(id).unwrap_or_default();
                // If the current book is missing in this translation, jump to its first.
                if !st.primary_books.iter().any(|b| b.nr == st.book)
                    && let Some(first) = st.primary_books.first()
                {
                    st.book = first.nr;
                    st.chapter = 1;
                }
            }
            refresh_books(&ui, &state.borrow());
            refresh_reading(&ui, &state);
            {
                let mut st = state.borrow_mut();
                let jobs = preload_jobs(&st);
                if !jobs.is_empty() {
                    let rx = spawn_preload(db_for_preload.clone(), jobs, false);
                    st.loading.push(rx);
                }
            }
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_secondary_changed(move |idx| {
            let ui = w.unwrap();
            let mut st = state.borrow_mut();
            let Some(&selected) = st.visible.get(idx as usize) else { return };
            st.secondary = selected;
            drop(st);
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_add_translation(move || {
            let ui = w.unwrap();
            {
                let mut st = state.borrow_mut();
                if st.extra.len() < 2 {
                    let selected: HashSet<usize> = std::iter::once(st.primary)
                        .chain(std::iter::once(st.secondary))
                        .chain(st.extra.iter().copied()).collect();
                    let candidate = st.visible.iter().copied().find(|i| !selected.contains(i)).unwrap_or(st.primary);
                    st.extra.push(candidate);
                }
                ui.set_extra_count(st.extra.len() as i32);
                update_extra_indices(&ui, &st);
            }
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_remove_translation(move || {
            let ui = w.unwrap();
            let mut st = state.borrow_mut();
            st.extra.pop();
            ui.set_extra_count(st.extra.len() as i32);
            drop(st);
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_extra_changed(move |slot, index| {
            let ui = w.unwrap();
            let mut st = state.borrow_mut();
            let actual = st.visible.get(index.max(0) as usize).copied();
            if let (Some(selected), Some(actual)) = (st.extra.get_mut(slot as usize), actual) {
                *selected = actual;
            }
            drop(st);
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        let bundled_db = logos_rs::importer::db_path();
        ui.on_import_docx(move |path, code, name, lang| {
            let ui = w.unwrap();
            let source = if bundled_db.is_file() {
                bundled_db.clone()
            } else {
                logos_rs::personal_import::personal_db_path().unwrap_or_default()
            };
            match logos_rs::personal_import::import_docx(
                &source, std::path::Path::new(path.as_str()),
                code.as_str(), name.as_str(), lang.as_str(),
            ) {
                Ok(count) => {
                    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                        let db = logos_rs::personal_import::personal_db_path()?;
                        let mut st = state.borrow_mut();
                        st.bible = Bible::open(&db)?;
                        st.translations = st.bible.translations()?;
                        st.visible = (0..st.translations.len()).filter(|&i|
                            !st.hidden.contains(&st.translations[i].code)
                            || st.primary == i || st.secondary == i || st.extra.contains(&i)
                        ).collect();
                        refresh_translation_menu(&ui, &st);
                        Ok(())
                    })();
                    match result {
                        Ok(()) => {
                            ui.set_import_status(format!("Imported {count} verses successfully.").into());
                            refresh_reading(&ui, &state);
                        },
                        Err(e) => ui.set_import_status(format!("Saved; restart app to load: {e}").into()),
                    }
                }
                Err(e) => ui.set_import_status(format!("Import failed: {e}").into()),
            }
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_toggle_translation_visibility(move |index| {
            let ui = w.unwrap();
            let mut st = state.borrow_mut();
            let i = index as usize;
            let Some(t) = st.translations.get(i) else { return };
            let code = t.code.clone();
            if st.primary == i || st.secondary == i || st.extra.contains(&i) { return; }
            if !st.hidden.insert(code.clone()) { st.hidden.remove(&code); }
            st.visible = (0..st.translations.len())
                .filter(|&i| !st.hidden.contains(&st.translations[i].code)).collect();
            refresh_translation_menu(&ui, &st);
            save_session(&ui, &st);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_toggle_parallel(move || {
            let ui = w.unwrap();
            ui.set_parallel(!ui.get_parallel());
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_toggle_reader(move || {
            let ui = w.unwrap();
            ui.set_reader(!ui.get_reader());
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, _state) = ctx!();
        ui.on_toggle_advanced(move || {
            let ui = w.unwrap();
            ui.set_advanced(!ui.get_advanced());
            if ui.get_advanced() {
                ui.set_focus_search(ui.get_focus_search() + 1);
            }
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_threshold_changed(move |idx| {
            let ui = w.unwrap();
            {
                let mut st = state.borrow_mut();
                st.threshold_idx = (idx as usize).min(THRESHOLDS.len() - 1);
                st.threshold_pinned = true;
            }
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_toggle_theme(move || {
            let ui = w.unwrap();
            let t = ui.global::<Theme>();
            t.set_dark(!t.get_dark());
            save_session(&ui, &state.borrow());
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_toggle_scroll_lock(move || {
            let ui = w.unwrap();
            ui.set_scroll_lock(!ui.get_scroll_lock());
            // Locked and unlocked are different views over different models,
            // so the chapter has to be rebuilt for the one now on screen.
            refresh_reading(&ui, &state);
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_go(move |text| {
            let ui = w.unwrap();
            let text = text.to_string();
            // Try to interpret as a reference first; fall back to full-text search.
            if navigate_reference(&state, &text) {
                ui.set_show_results(false);
                refresh_reading(&ui, &state);
            } else {
                run_search(&ui, &state, &text);
            }
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_open_result(move |idx| {
            let ui = w.unwrap();
            let hit = state.borrow().last_results.get(idx as usize).cloned();
            if let Some(hit) = hit {
                {
                    let mut st = state.borrow_mut();
                    st.book = hit.book;
                    st.chapter = hit.chapter;
                    st.target_verse = Some(hit.verse);
                }
                // The list stays loaded behind the text: the "‹ Results" pill
                // brings it back.
                ui.set_show_results(false);
                refresh_reading(&ui, &state);
            }
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_close_results(move || {
            let ui = w.unwrap();
            ui.set_show_results(false);
            ui.set_results(ModelRc::new(VecModel::default()));
            state.borrow_mut().last_results.clear();
        });
    }
    {
        let (w, state) = ctx!();
        ui.on_word_clicked(move |col, idx| {
            let ui = w.unwrap();
            let st = state.borrow();
            let tindex = match col {
                0 => st.primary,
                1 => st.secondary,
                n => match st.extra.get((n - 2) as usize) { Some(i) => *i, None => return },
            };
            let trans = st.translations[tindex].clone();
            let words = match col {
                0 => &st.primary_words,
                1 => &st.secondary_words,
                n => match st.extra_words.get((n - 2) as usize) { Some(w) => w, None => return },
            };
            let Some(word) = words.get(idx as usize).cloned() else { return };

            // A modern-language translation (KJV/ULT/LSG…) — as opposed to the
            // original-language texts (Hebrew/Greek), whose words ARE the original.
            let is_translation = trans.lang != "hbo" && trans.lang != "grc";
            let is_ot = word.book < 40;
            let orig_rtl = is_ot; // Hebrew is RTL, Greek LTR

            // Reverse lookup: clicking a translation word resolves it to its
            // original-language word(s) in the same verse.
            let mut originals_label = String::new();
            let mut originals: Vec<OriginalWord> = Vec::new();
            let mut parsing = word.parsing.clone();
            if is_translation && !word.strongs.is_empty() {
                let orig_code = if is_ot { "WLC" } else { "SBLGNT" };
                if let Some(ot) = st.translations.iter().find(|t| t.code == orig_code) {
                    let ws = st
                        .bible
                        .words_with_strongs(ot.id, word.book, word.chapter, word.verse, &word.strongs)
                        .unwrap_or_default();
                    if parsing.is_empty() && let Some(first) = ws.first() {
                        parsing = first.parsing.clone();
                    }
                    originals = ws
                        .iter()
                        .map(|o| OriginalWord {
                            text: o.text.clone().into(),
                            parsing: o.parsing.clone().into(),
                        })
                        .collect();
                    originals_label = format!(
                        "In the {} ({orig_code})",
                        if is_ot { "Hebrew" } else { "Greek" }
                    );
                }
            }

            // Septuagint verse for OT words.
            let mut lxx = String::new();
            let mut show_lxx = false;
            if is_ot
                && let Some(lt) = st.translations.iter().find(|t| t.code == "LXX")
                && let Ok(Some(text)) =
                    st.bible.verse_text(lt.id, word.book, word.chapter, word.verse)
            {
                lxx = text;
                show_lxx = true;
            }

            // Vulgate (Latin) verse reference — for both OT and NT words.
            let mut vulgate = String::new();
            let mut show_vulgate = false;
            if let Some(vt) = st.translations.iter().find(|t| t.code == "VUL")
                && let Ok(Some(text)) =
                    st.bible.verse_text(vt.id, word.book, word.chapter, word.verse)
            {
                vulgate = text;
                show_vulgate = true;
            }

            ui.set_word(WordInfo {
                surface: word.text.clone().into(),
                translit: word.translit.clone().into(),
                lemma: word.lemma.clone().into(),
                strongs: word.strongs.clone().into(),
                parsing: parsing.into(),
                gloss: word.gloss.clone().into(),
                definition: word.definition.clone().into(),
                rtl: trans.rtl,
                orig_rtl,
                originals_label: originals_label.into(),
                originals: ModelRc::new(VecModel::from(originals)),
                lxx: lxx.into(),
                show_lxx,
                vulgate: vulgate.into(),
                show_vulgate,
            });
            ui.set_selected_col(col);
            ui.set_selected_idx(idx);
            ui.set_show_word(true);
        });
    }
    {
        let (w, _state) = ctx!();
        ui.on_close_word(move || {
            let ui = w.unwrap();
            ui.set_show_word(false);
            ui.set_selected_col(-1);
            ui.set_selected_idx(-1);
        });
    }

    // Closing is the one moment the final window size is known, and the
    // backstop for anything toggled since the last refresh.
    {
        let (w, state) = ctx!();
        ui.window().on_close_requested(move || {
            if let Some(ui) = w.upgrade() {
                save_session(&ui, &state.borrow());
            }
            slint::CloseRequestResponse::HideWindow
        });
    }

    ui.run()?;
    Ok(())
}

/// Put the window back the way it was left, before the first refresh reads
/// these flags.
fn menu_index(st: &State, actual: usize) -> i32 {
    st.visible.iter().position(|&i| i == actual).unwrap_or(0) as i32
}

fn refresh_translation_menu(ui: &MainWindow, st: &State) {
    let labels: Vec<SharedString> = st.visible.iter().map(|&i| {
        let t = &st.translations[i];
        format!("{} — {}", t.code, t.name).into()
    }).collect();
    ui.set_translation_labels(ModelRc::new(VecModel::from(labels)));
    let options: Vec<SharedString> = st.translations.iter().map(|t| {
        format!("{} {} — {}", if st.hidden.contains(&t.code) { "○" } else { "✓" }, t.code, t.name).into()
    }).collect();
    ui.set_visibility_options(ModelRc::new(VecModel::from(options)));
    ui.set_primary_index(menu_index(st, st.primary));
    ui.set_secondary_index(menu_index(st, st.secondary));
    update_extra_indices(ui, st);
}

fn update_extra_indices(ui: &MainWindow, st: &State) {
    ui.set_third_index(menu_index(st, st.extra.first().copied().unwrap_or(st.primary)));
    ui.set_fourth_index(menu_index(st, st.extra.get(1).copied().unwrap_or(st.primary)));
}

fn restore(ui: &MainWindow, saved: &Session, primary: usize, secondary: usize) {
    ui.global::<Theme>().set_dark(saved.dark);
    ui.set_parallel(saved.parallel);
    ui.set_scroll_lock(saved.scroll_lock);
    ui.set_reader(saved.reader);
    ui.set_primary_index(primary as i32);
    ui.set_secondary_index(secondary as i32);
    // Guard against a degenerate saved size — a window restored at 1×1 would
    // be unrecoverable without editing the file by hand.
    if let (Some(w), Some(h)) = (saved.width, saved.height)
        && w >= 640
        && h >= 400
    {
        ui.window().set_size(slint::LogicalSize::new(w as f32, h as f32));
    }
}

/// Persist the session — unless the window has not been shown yet, when it
/// reports a size of 0×0 that would overwrite the one saved last run. Nothing
/// has changed at that point anyway.
fn save_session(ui: &MainWindow, st: &State) {
    if ui.window().size().width == 0 {
        return;
    }
    session::save(&snapshot(ui, st));
}

/// Everything worth carrying into the next launch.
fn snapshot(ui: &MainWindow, st: &State) -> Session {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    Session {
        primary: st.translations[st.primary].code.clone(),
        secondary: st.translations[st.secondary].code.clone(),
        extra_translations: st.extra.iter().map(|&i| st.translations[i].code.clone()).collect(),
        hidden_translations: st.hidden.iter().cloned().collect(),
        book: st.book,
        chapter: st.chapter,
        parallel: ui.get_parallel(),
        scroll_lock: ui.get_scroll_lock(),
        reader: ui.get_reader(),
        // Only a hand-picked cut-off is worth restoring; otherwise each corpus
        // keeps its own conventional default.
        threshold: st.threshold_pinned.then(|| THRESHOLDS[st.threshold_idx]),
        dark: ui.global::<Theme>().get_dark(),
        width: Some(size.width as u32),
        height: Some(size.height as u32),
    }
}

/// Build the sidebar book list with Old/New/Deuterocanon section headers.
fn refresh_books(ui: &MainWindow, st: &State) {
    let mut items: Vec<BookItem> = Vec::new();
    let mut last_section = -1i64;
    for b in &st.primary_books {
        if b.section != last_section {
            last_section = b.section;
            items.push(BookItem {
                nr: -1,
                name: Section::of(if b.section == 0 { 1 } else if b.section == 1 { 40 } else { 100 })
                    .label()
                    .into(),
                header: true,
            });
        }
        items.push(BookItem { nr: b.nr as i32, name: b.name.clone().into(), header: false });
    }
    ui.set_books(ModelRc::new(VecModel::from(items)));
}

/// Refresh chapter grid, verse columns and titles for the current location.
fn refresh_reading(ui: &MainWindow, state: &Rc<RefCell<State>>) {
    let mut st = state.borrow_mut();
    let pid = st.primary_id();
    let count = st.bible.chapter_count(pid, st.book).unwrap_or(0).max(1);
    let chapter = st.chapter.clamp(1, count);
    st.chapter = chapter;

    let chapters: Vec<i32> = (1..=count as i32).collect();
    ui.set_chapters(ModelRc::new(VecModel::from(chapters)));
    ui.set_current_book(st.book as i32);
    ui.set_current_chapter(chapter as i32);

    let parallel = ui.get_parallel();
    let (book, primary, secondary) = (st.book, st.primary, st.secondary);

    // ----- primary column -----
    let primary_rtl = st.translations[primary].rtl;
    let lang = st.translations[primary].lang.clone();
    let reader_capable = st.translations[primary].tagged && is_original_language(&lang);

    // Each corpus has its own conventional cut-off, so follow the text unless
    // the reader has chosen one explicitly.
    if !st.threshold_pinned && st.threshold_lang != lang {
        st.threshold_idx = default_threshold(&lang);
        st.threshold_lang = lang.clone();
        ui.set_threshold_index(st.threshold_idx as i32);
    }
    ui.set_primary_title(
        format!("{} {} · {}", st.book_name(book), chapter, st.translations[primary].code).into(),
    );
    ui.set_primary_subtitle(st.translations[primary].name.clone().into());

    // The reader's apparatus needs whole-corpus counts; build them once per
    // translation, and only when the reader actually asks for them.
    ui.set_reader_available(reader_capable);
    let reader_on = reader_capable && ui.get_reader();
    if reader_on && st.freq_tid != pid {
        let freq = st.bible.lexeme_frequencies(pid).unwrap_or_default();
        st.freq = freq;
        st.freq_tid = pid;
    }

    let primary_rows: Vec<VerseRow>;
    if st.translations[primary].tagged {
        let max_freq = THRESHOLDS[st.threshold_idx.min(THRESHOLDS.len() - 1)];
        let reader = reader_on.then(|| Reader { freq: &st.freq, max_freq });
        let (rows, flat) =
            tagged_rows(&st.bible, pid, book, chapter, primary_rtl, reader.as_ref());
        primary_rows = rows;
        if let Some(r) = &reader {
            let notes = gloss_notes(&flat, r, primary_rtl);
            ui.set_reader_caption(
                reader_caption(&st.translations[primary], max_freq, notes.len()).into(),
            );
            ui.set_notes(ModelRc::new(VecModel::from(notes)));
        }
        st.primary_words = flat;
    } else {
        let verses = st.bible.chapter(pid, book, chapter).unwrap_or_default();
        primary_rows = plain_rows(&verses, primary_rtl);
        st.primary_words.clear();
    }
    if !reader_on {
        ui.set_notes(ModelRc::new(VecModel::default()));
    }

    // ----- secondary column (parallel only) -----
    let sid = st.secondary_id();
    let secondary_rtl = st.translations[secondary].rtl;
    ui.set_secondary_title(st.translations[secondary].name.clone().into());
    let mut secondary_rows: Vec<VerseRow> = Vec::new();
    if !parallel {
        // nothing to lay out when the second column is not on screen
        st.secondary_words.clear();
    } else if st.translations[secondary].tagged {
        let (rows, flat) = tagged_rows(&st.bible, sid, book, chapter, secondary_rtl, None);
        st.secondary_words = flat;
        secondary_rows = rows;
    } else {
        let verses2 = st.bible.chapter(sid, book, chapter).unwrap_or_default();
        secondary_rows = plain_rows(&verses2, secondary_rtl);
        st.secondary_words.clear();
    }

    // Linked reading uses one row per verse across every open translation.
    ui.set_third_title(st.extra.first().map(|&i| st.translations[i].name.as_str()).unwrap_or("").into());
    ui.set_fourth_title(st.extra.get(1).map(|&i| st.translations[i].name.as_str()).unwrap_or("").into());
    // All open translations participate in the same verse-aligned model.
    let mut extra_rows = Vec::new();
    for slot in 0..2 {
        let mut rows = Vec::new();
        let choice = if parallel { st.extra.get(slot).copied() } else { None };
        if let Some(index) = choice {
            let t = &st.translations[index];
            if t.tagged {
                let (r, flat) = tagged_rows(&st.bible, t.id, book, chapter, t.rtl, None);
                rows = r;
                st.extra_words[slot] = flat;
            } else {
                rows = plain_rows(&st.bible.chapter(t.id, book, chapter).unwrap_or_default(), t.rtl);
                st.extra_words[slot].clear();
            }
        } else {
            st.extra_words[slot].clear();
        }
        extra_rows.push(rows.clone());
        if slot == 0 { ui.set_rows3(ModelRc::new(VecModel::from(rows))); }
        else { ui.set_rows4(ModelRc::new(VecModel::from(rows))); }
    }
    let locked = parallel && ui.get_scroll_lock();
    // A search hit lands on its verse: hand the view the row index (rows are
    // in verse order but a translation may skip a verse, so look it up), or
    // -1 for the top of the chapter.
    let target = st.target_verse.take().map(|v| v as i32);
    if locked {
        let pairs = pair_rows(&primary_rows, &secondary_rows, &extra_rows);
        ui.set_target_index(row_index(pairs.iter().map(|p| p.verse), target));
        ui.set_pairs(ModelRc::new(VecModel::from(pairs)));
        ui.set_rows(ModelRc::new(VecModel::default()));
        ui.set_rows2(ModelRc::new(VecModel::default()));
    } else {
        ui.set_target_index(row_index(primary_rows.iter().map(|r| r.verse), target));
        ui.set_pairs(ModelRc::new(VecModel::default()));
        ui.set_rows(ModelRc::new(VecModel::from(primary_rows)));
        ui.set_rows2(ModelRc::new(VecModel::from(secondary_rows)));
    }

    // clear word-selection highlight (indices change with the chapter)
    ui.set_selected_col(-1);
    ui.set_selected_idx(-1);
    // send both columns back to the top of the new passage
    ui.set_nav_epoch(ui.get_nav_epoch() + 1);

    // Every navigation, translation change and pane toggle lands here, which
    // makes it the one place the session needs writing from.
    save_session(ui, &st);
}

/// Build word rows for a tagged translation, plus the flat word list
/// (indexed by the chip `idx` used for click lookups).
fn tagged_rows(
    bible: &Bible,
    tid: i64,
    book: i64,
    chapter: i64,
    rtl: bool,
    reader: Option<&Reader>,
) -> (Vec<VerseRow>, Vec<Word>) {
    let verses = bible.chapter(tid, book, chapter).unwrap_or_default();
    let mut flat: Vec<Word> = Vec::new();
    let mut blocks: Vec<VerseRow> = Vec::new();

    for v in &verses {
        let words = bible.words(tid, book, chapter, v.verse).unwrap_or_default();
        let base = flat.len();
        // Keep canonical order; the UI wraps measured word chips to the
        // actual cell width, including RTL and window/column size changes.
        let chips: Vec<WordChip> = words.iter().enumerate().map(|(i, w)| WordChip {
            idx: (base + i) as i32,
            text: w.text.clone().into(),
            tagged: !w.strongs.is_empty(),
            rare: reader.is_some_and(|r| r.glosses(w)),
        }).collect();
        flat.extend(words);
        blocks.push(VerseRow {
            verse: v.verse as i32,
            text: SharedString::default(),
            words: ModelRc::new(VecModel::from(chips)),
            tagged: true,
            rtl,
        });
    }
    (blocks, flat)
}

/// The reader's-edition apparatus for one chapter: every lexeme rarer than the
/// chosen cut-off, in reading order.
///
/// A word is listed once per verse, as printed reader's editions do — a rare
/// word repeated within a verse is looked up once, but a reader returning to it
/// in a later verse should not have to hunt back up the page.
fn gloss_notes(words: &[Word], reader: &Reader, rtl: bool) -> Vec<GlossItem> {
    let mut seen: HashSet<(i64, &str)> = HashSet::new();
    let mut notes: Vec<GlossItem> = Vec::new();
    for w in words {
        if !reader.glosses(w) || !seen.insert((w.verse, &w.strongs)) {
            continue;
        }
        notes.push(GlossItem {
            verse: w.verse as i32,
            surface: w.text.clone().into(),
            lemma: w.lemma.clone().into(),
            parsing: w.parsing.clone().into(),
            gloss: short_gloss(&w.gloss, &w.definition).into(),
            count: format!("{}×", reader.count(&w.strongs)).into(),
            rtl,
        });
    }
    notes
}

/// Shorten a lexicon entry to the senses that fit one apparatus line. Falls
/// back to the long definition for the entries that carry no short gloss.
fn short_gloss(gloss: &str, definition: &str) -> String {
    let source = if gloss.trim().is_empty() { definition } else { gloss };
    // The Hebrew entries separate senses with semicolons and the Greek ones
    // with commas, so split on whichever this entry actually uses.
    let sep = if source.contains(';') { ';' } else { ',' };
    let senses: Vec<&str> =
        source.split(sep).map(str::trim).filter(|s| !s.is_empty()).take(3).collect();
    let joined = senses.join(", ");
    match joined.char_indices().nth(64) {
        Some((cut, _)) => format!("{}…", joined[..cut].trim_end()),
        None => joined,
    }
}

/// The line above the apparatus, naming the corpus the counts are taken over.
///
/// Deliberately free of the book name: the header already carries it, and a
/// Hebrew name opening a Latin sentence flips the whole line under the
/// bidirectional text algorithm.
fn reader_caption(trans: &Translation, max_freq: i64, entries: usize) -> String {
    let corpus = match trans.lang.as_str() {
        "hbo" => "the Hebrew Bible".to_string(),
        "grc" => "the Greek New Testament".to_string(),
        _ => trans.name.clone(),
    };
    format!(
        "{entries} word{} occurring fewer than {max_freq}× in {corpus}",
        if entries == 1 { "" } else { "s" }
    )
}

/// Build plain prose rows for an untagged translation.
fn plain_rows(verses: &[bible::Verse], rtl: bool) -> Vec<VerseRow> {
    verses
        .iter()
        .map(|v| VerseRow {
            verse: v.verse as i32,
            text: v.text.clone().into(),
            words: ModelRc::new(VecModel::default()),
            tagged: false,
            rtl,
        })
        .collect()
}

/// Position of `verse` among the rows a view shows, or -1 for none.
fn row_index(verses: impl Iterator<Item = i32>, verse: Option<i32>) -> i32 {
    match verse {
        Some(v) => verses.enumerate().find(|(_, x)| *x == v).map_or(-1, |(i, _)| i as i32),
        None => -1,
    }
}

/// Pair the two translations verse by verse for the locked view.
///
/// Matched on verse number rather than position: versification differs between
/// translations, so one may carry a verse the other does not. A missing side
/// becomes an empty cell (`VerseRow::default()`, whose verse is 0) and the
/// rest of the chapter stays aligned.
fn pair_rows(left: &[VerseRow], right: &[VerseRow], extra: &[Vec<VerseRow>]) -> Vec<PairRow> {
    let columns: Vec<&[VerseRow]> = std::iter::once(left).chain(std::iter::once(right))
        .chain(extra.iter().map(Vec::as_slice)).collect();
    let mut verses: Vec<i32> = columns.iter().flat_map(|rows| rows.iter().map(|r| r.verse)).collect();
    verses.sort_unstable();
    verses.dedup();
    let cell = |col: usize, verse: i32| {
        columns.get(col).and_then(|rows| rows.iter().find(|r| r.verse == verse))
            .cloned().unwrap_or_default()
    };
    verses.into_iter().map(|verse| PairRow {
        verse, left: cell(0, verse), right: cell(1, verse),
        third: cell(2, verse), fourth: cell(3, verse),
    }).collect()
}

/// Move the current chapter by `delta`, rolling over to adjacent books.
fn step_chapter(state: &Rc<RefCell<State>>, delta: i64) {
    let mut st = state.borrow_mut();
    let pid = st.primary_id();
    let count = st.bible.chapter_count(pid, st.book).unwrap_or(1).max(1);
    let next = st.chapter + delta;
    if next >= 1 && next <= count {
        st.chapter = next;
        return;
    }
    // roll over to an adjacent book that exists in this translation
    let books: Vec<i64> = st.primary_books.iter().map(|b| b.nr).collect();
    let pos = books.iter().position(|&n| n == st.book);
    if let Some(pos) = pos {
        if delta > 0 && pos + 1 < books.len() {
            st.book = books[pos + 1];
            st.chapter = 1;
        } else if delta < 0 && pos > 0 {
            let prev = books[pos - 1];
            st.book = prev;
            st.chapter = st.bible.chapter_count(pid, prev).unwrap_or(1).max(1);
        }
    }
}

/// Try to parse `text` as a reference and move there. Returns false if it
/// isn't a recognisable reference (so the caller can search instead).
fn navigate_reference(state: &Rc<RefCell<State>>, text: &str) -> bool {
    let Some(r) = reference::parse(text) else { return false };
    let mut st = state.borrow_mut();
    // The book must exist in the primary translation.
    if !st.primary_books.iter().any(|b| b.nr == r.book) {
        return false;
    }
    st.book = r.book;
    st.chapter = r.chapter.unwrap_or(1);
    true
}

/// Search. Plain words go to the full-text index of the translation being
/// read; anything using the query language (fields, `[…]`, Strong's numbers,
/// operators, morphology keywords) goes to the advanced engine.
fn run_search(ui: &MainWindow, state: &Rc<RefCell<State>>, query: &str) {
    if search::is_advanced(query) {
        run_advanced(ui, state, query);
        return;
    }
    let hits = {
        let st = state.borrow();
        // In canonical order a small cap would cut the list off in Genesis
        // for a common word; the list view is lazy, so this costs nothing.
        st.bible.search(st.primary_id(), query, 5000).unwrap_or_default()
    };
    let items: Vec<SearchItem> = hits
        .iter()
        .map(search_item)
        .collect();
    let n = items.len();
    ui.set_results(ModelRc::new(VecModel::from(items)));
    ui.set_results_summary(
        format!("{n} result{} for “{query}”", if n == 1 { "" } else { "s" }).into(),
    );
    ui.set_show_results(true);
    // a new query starts at the top of the list; coming back from a hit does not
    ui.set_search_epoch(ui.get_search_epoch() + 1);
    state.borrow_mut().last_results = hits;
}

/// Advanced search: evaluate the query over the original-language text —
/// the Hebrew Bible for the OT, the Greek NT for the NT — and show the hits
/// in the translation being read, marking the words that carry the matched
/// Strong's numbers. Searching in Greek and reading the hits in French is the
/// point; it is also more complete than a translation's own tags.
fn run_advanced(ui: &MainWindow, state: &Rc<RefCell<State>>, query: &str) {
    let started = std::time::Instant::now();
    let node = match search::parse(query) {
        Ok(n) => n,
        Err(e) => {
            ui.set_results(ModelRc::new(VecModel::default()));
            ui.set_results_summary(format!("Cannot read that query: {e}").into());
            ui.set_show_results(true);
            state.borrow_mut().last_results.clear();
            return;
        }
    };

    let mut st = state.borrow_mut();
    let lexicon = st.lexicon();
    let primary = st.primary_id();
    let mut loaded_ms: u128 = 0;
    let t0 = std::time::Instant::now();
    let target_words: Rc<Corpus> = st.corpus(primary);
    loaded_ms += t0.elapsed().as_millis();
    let originals: Vec<(i64, &str)> = [("WLC", "the Hebrew Bible"), ("SBLGNT", "the Greek New Testament")]
        .iter()
        .filter_map(|(code, label)| st.translation_by_code(code).map(|t| (t.id, *label)))
        .collect();

    let mut hits: Vec<SearchHit> = Vec::new();
    let mut searched: Vec<&str> = Vec::new();
    for (tid, label) in originals {
        let t0 = std::time::Instant::now();
        let corpus = st.corpus(tid);
        loaded_ms += t0.elapsed().as_millis();
        let compiled = search::Compiled::new(&node, &corpus, &lexicon);
        let found = compiled.run(&corpus);
        if !found.is_empty() {
            searched.push(label);
        }
        for h in found {
            let matched: HashSet<u32> = h.positions.iter().map(|&i| corpus.words[i].strongs).collect();
            let snippet = if tid == primary {
                // reading the original itself: mark the matched words
                mark_words(&corpus, &h.verse, |i, _| h.positions.contains(&i))
            } else {
                // reading a translation: mark its words that carry a matched Strong's
                let names: HashSet<&str> = matched.iter().map(|&s| corpus.strongs.name(s)).collect();
                match target_words.span(h.verse) {
                    Some(_) => mark_words(&target_words, &h.verse, |_, w| {
                        w.strongs != 0 && names.contains(target_words.strongs.name(w.strongs))
                    }),
                    // The translation has no such verse (versification differs):
                    // show the original rather than lose the hit.
                    None => mark_words(&corpus, &h.verse, |i, _| h.positions.contains(&i)),
                }
            };
            hits.push(SearchHit {
                book: h.verse.book,
                book_name: st.book_name(h.verse.book),
                chapter: h.verse.chapter,
                verse: h.verse.verse,
                snippet,
            });
        }
    }
    hits.sort_by_key(|h| (h.book, h.chapter, h.verse));

    let items: Vec<SearchItem> = hits
        .iter()
        .map(search_item)
        .collect();
    let n = items.len();
    let scope = if searched.is_empty() { "the original text".to_string() } else { searched.join(" and ") };
    let took = started.elapsed().as_millis();
    let note = if loaded_ms > 50 { format!(" ({loaded_ms} ms of it loading the text, once)") } else { String::new() };
    ui.set_results(ModelRc::new(VecModel::from(items)));
    ui.set_results_summary(
        format!(
            "{n} result{} for “{query}” in {scope} · {took} ms{note}",
            if n == 1 { "" } else { "s" }
        )
        .into(),
    );
    ui.set_show_results(true);
    ui.set_search_epoch(ui.get_search_epoch() + 1);
    st.last_results = hits;
}

/// What the background loader hands back to the UI thread.
enum Preloaded {
    Corpus(i64, Box<Corpus>),
    Lexicon(Lexicon),
}

/// Read and build corpora on another thread, over its own connection, so the
/// first advanced search does not pay several seconds for it. Results come
/// back over a channel the UI thread drains on a timer.
fn spawn_preload(
    db: std::path::PathBuf,
    jobs: Vec<(i64, bool)>,
    lexicon: bool,
) -> std::sync::mpsc::Receiver<Preloaded> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok(bible) = Bible::open(&db) else { return };
        if lexicon && let Ok(rows) = bible.lexicon_rows() {
            let _ = tx.send(Preloaded::Lexicon(Lexicon::new(rows)));
        }
        for (tid, hebrew) in jobs {
            if let Ok(rows) = bible.corpus_words(tid) {
                let _ = tx.send(Preloaded::Corpus(tid, Box::new(Corpus::build(rows, hebrew))));
            }
        }
    });
    rx
}

/// Corpora the advanced search will need for the current reading setup that
/// are not loaded yet: the two originals and the translation being read.
fn preload_jobs(st: &State) -> Vec<(i64, bool)> {
    let mut jobs = Vec::new();
    for code in ["WLC", "SBLGNT"] {
        if let Some(t) = st.translation_by_code(code) {
            jobs.push((t.id, t.lang == "hbo"));
        }
    }
    let pid = st.primary_id();
    if !jobs.iter().any(|(id, _)| *id == pid) {
        jobs.push((pid, false));
    }
    jobs.retain(|(id, _)| !st.corpora.contains_key(id));
    jobs
}

/// A results row from a hit: the marked snippet as highlighted word chips,
/// pre-wrapped into lines (Slint has no inline styling and no flow layout).
fn search_item(h: &SearchHit) -> SearchItem {
    let (lines, rtl) = search::hit_lines(&h.snippet, HIT_OPEN, HIT_CLOSE, 105);
    let lines: Vec<HitLine> = lines
        .into_iter()
        .map(|ws| HitLine {
            words: ModelRc::new(VecModel::from(
                ws.into_iter().map(|(text, hit)| HitWord { text: text.into(), hit }).collect::<Vec<_>>(),
            )),
        })
        .collect();
    SearchItem {
        label: format!("{} {}:{}", h.book_name, h.chapter, h.verse).into(),
        snippet: h.snippet.clone().into(),
        lines: ModelRc::new(VecModel::from(lines)),
        rtl,
        book: h.book as i32,
        chapter: h.chapter as i32,
        verse: h.verse as i32,
    }
}

/// A verse's words joined into a line, the chosen ones wrapped in the hit
/// markers (the same convention the full-text snippets use).
fn mark_words(corpus: &Corpus, verse: &VerseId, pick: impl Fn(usize, &search::Word) -> bool) -> String {
    let Some(span) = corpus.span(*verse) else { return String::new() };
    (span.start..span.end)
        .map(|i| {
            let text = &corpus.display[i];
            if pick(i, &corpus.words[i]) { format!("{HIT_OPEN}{text}{HIT_CLOSE}") } else { text.clone() }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use i_slint_backend_testing::{ElementQuery, TestingBackend, TestingBackendOptions};

    fn verse(n: i32, text: &str, tagged: bool, rtl: bool) -> VerseRow {
        VerseRow {
            verse: n, text: text.into(), tagged, rtl,
            words: ModelRc::new(VecModel::from(text.split_whitespace().enumerate()
                .map(|(idx, text)| WordChip { idx: idx as i32, text: text.into(),
                    tagged: true, rare: false }).collect::<Vec<_>>())),
        }
    }

    #[test]
    fn linked_rows_include_verses_from_every_column() {
        let left = vec![verse(1, "one", false, false)];
        let right = vec![verse(2, "two", false, false)];
        let extra = vec![vec![verse(3, "three", false, false)], vec![verse(4, "four", false, false)]];
        let rows = pair_rows(&left, &right, &extra);
        assert_eq!(rows.iter().map(|r| r.verse).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
        assert_eq!(rows[2].third.text.as_str(), "three");
        assert_eq!(rows[3].fourth.text.as_str(), "four");
        assert_eq!(rows[3].left.verse, 0);
        assert_eq!(rows[0].fourth.verse, 0);
    }

    #[test]
    fn text_fits_and_linked_columns_scroll_together() {
        slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
            renderer_name: Some("software".into()), ..Default::default()
        }))).unwrap();
        let ui = MainWindow::new().unwrap();
        ui.set_parallel(true);
        ui.set_scroll_lock(true);
        let text = "In the beginning God created the heavens and the earth. ".repeat(6);
        let hebrew = "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ ".repeat(6);
        let plain: Vec<_> = (1..=20).map(|n| verse(n, &text, false, false)).collect();
        let tagged: Vec<_> = (1..=20).map(|n| verse(n, &text, true, false)).collect();
        let rtl: Vec<_> = (1..=20).map(|n| verse(n, &hebrew, true, true)).collect();
        ui.show().unwrap();
        for count in 2..=4 {
            ui.set_extra_count(count - 2);
            ui.set_pairs(ModelRc::new(VecModel::from(pair_rows(&plain, &tagged,
                &[rtl.clone(), plain.clone()]))));
            for (width, word_panel) in [(720., false), (1200., false), (1600., false), (1200., true), (720., true)] {
                ui.set_show_word(word_panel);
                ui.window().set_size(slint::LogicalSize::new(width, 900.));
                let snapshot = ui.window().take_snapshot().unwrap();
                if let Ok(dir) = std::env::var("LAYOUT_SNAPSHOTS") {
                    std::fs::create_dir_all(&dir).unwrap();
                    let mut ppm = format!("P6\n{} {}\n255\n", snapshot.width(), snapshot.height()).into_bytes();
                    for pixel in snapshot.as_bytes().chunks_exact(4) { ppm.extend_from_slice(&pixel[..3]); }
                    std::fs::write(format!("{dir}/columns-{count}-width-{width}-panel-{word_panel}.ppm"), ppm).unwrap();
                }
                let scrollers = ElementQuery::from_root(&ui).match_inherits("PairedColumn").find_all();
                assert_eq!(scrollers.len(), 1, "all linked Bibles must use one scroller");
                assert!(ElementQuery::from_root(&ui).match_inherits("ReadingColumn").find_all().is_empty());
                let cells = ElementQuery::from_root(&ui).match_inherits("VerseCell").find_all();
                assert!(!cells.is_empty());
                for cell in &cells {
                    let pos = cell.absolute_position();
                    let size = cell.size();
                    // Inactive third/fourth cells are outside the live grid.
                    if pos.x >= width || size.width <= 0. { continue; }
                    for t in cell.query_descendants().match_inherits("Text").find_all() {
                        let tp = t.absolute_position();
                        assert!(tp.x >= pos.x - 1. && tp.x + t.size().width <= pos.x + size.width + 1.,
                            "text overflows cell: window={width}, columns={count}, cell={pos:?}/{size:?}, text={tp:?}/{:?}", t.size());
                    }
                }
                // A wheel gesture over any column moves the entire verse grid.
                let first = ElementQuery::from_root(&ui).match_id("PairedColumn::lcell").find_first().unwrap();
                let before = first.absolute_position().y;
                scrollers[0].scroll(0., -120.);
                ui.window().take_snapshot().unwrap();
                let after = first.absolute_position().y;
                assert!(after < before, "linked wheel scrolling did not move verses");
                for id in ["PairedColumn::rcell", "PairedColumn::tcell", "PairedColumn::fcell"].iter().take(count as usize - 1) {
                    let other = ElementQuery::from_root(&ui).match_id(*id).find_first().unwrap();
                    assert!((other.absolute_position().y - after).abs() < 1.);
                }
                ui.set_nav_epoch(ui.get_nav_epoch() + 1);
            }
        }
        ui.set_scroll_lock(false);
        ui.set_rows(ModelRc::new(VecModel::from(plain.clone())));
        ui.set_rows2(ModelRc::new(VecModel::from(plain.clone())));
        ui.set_rows3(ModelRc::new(VecModel::from(plain.clone())));
        ui.set_rows4(ModelRc::new(VecModel::from(plain)));
        ui.window().take_snapshot().unwrap();
        assert!(ElementQuery::from_root(&ui).match_inherits("PairedColumn").find_all().is_empty());
        assert_eq!(ElementQuery::from_root(&ui).match_inherits("ReadingColumn").find_all().len(), 4);
    }
}
