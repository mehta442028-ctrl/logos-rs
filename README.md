# logos-rs

A Linux desktop Bible study app inspired by [Logos](https://www.logos.com/),
built with **Rust** + **[Slint](https://slint.dev/)**. The goal is a Logos-like
study experience that runs natively on Linux with a cleaner, faster UX.

![reading view](docs/reading.png)

## Features

- **Reading view** with hanging-indent verse numbers and a Logos-style three-pane
  layout (translation/book/chapter nav · reading pane).
- **Parallel view** — read two to four translations side by side, aligned by verse:
  locked (the default) renders all open texts as a *single* list of aligned verses,
  so verse N always sits beside verse N and the columns cannot drift apart. The
  **🔗 Linked** pill switches them to independent scrollers. Text wraps to each column as the window is resized.
- **Search** — `Ctrl+F` jumps to the search box from anywhere. Hits are listed
  in canonical order (Genesis → Revelation), and opening one lands on that verse,
  tinted, rather than at the top of its chapter; **☰ Results** in the header
  brings the list back, **✕ Close** dismisses it.
- **Advanced search** — `Ctrl+Shift+F` (or **⌕ Advanced**) opens a reference card
  for a small query language, typed into the same box. It runs over the tagged
  Hebrew OT / Greek NT and shows the hits in whatever translation you are
  reading, with the matching words marked — search in Greek, read the hits in
  French. Whole-corpus, in memory, typically 20–150 ms.

  ```text
  G25   H430   lemma:ἀγαπάω   gloss:love      one word, by Strong's, lemma or gloss
  [lemma:ἀγαπάω aorist active]                constraints on the SAME word
  tense: voice: mood: case: number: gender: person: stem: state: — or the bare value
  A B · A AND B · A OR B · NOT A · ( … )      per verse
  A BEFORE/2 B · A AFTER/1 B · A NEAR/3 B    word order, within n words
  [verb aorist] BEFORE/1 lemma:θεός           an aorist verb right before "God"
  ```
- **Picks up where you left off** — the passage, both translations, which panes
  were open, the theme and the window size are restored on the next launch
  (`~/.config/logos-rs/session.json`, or `$LOGOS_SESSION`).
- **Reader's edition** — open the Hebrew OT (WLC) or Greek NT (SBLGNT) and hit
  **📖 Reader**: every word rarer than the chosen cut-off is ruled in the text and
  glossed below it with its dictionary form, full parsing and corpus frequency, so
  a chapter can be read straight through without a lexicon. Defaults follow the
  printed editions — fewer than 100× for the Hebrew Bible, fewer than 30× for the
  Greek NT — and proper names are left out, as they are in print. No complete
  parsed Hebrew OT exists in book or PDF form; this is that, generated per chapter.
- **Word study** — in the original-language texts (Hebrew OT, Greek NT) every word
  is clickable. A side panel shows the surface form, transliteration, lemma,
  Strong's number, full morphological parsing, gloss, and definition — inspired by
  [hb.openscriptures.org](https://hb.openscriptures.org/) and
  [biblewebapp.com](https://biblewebapp.com/reader/).
- **Reverse interlinear** — click a word in **KJV**, **ULT**, **Louis Segond** (or the **ESV**, when present) and the panel shows
  the original word it translates: the Greek for the NT, and the Hebrew **plus the
  parallel Septuagint verse** for the OT (lemma, Strong's, parsing, definition).
  The parallel **Vulgate (Latin)** verse is shown as a reference for both.
- **Localized book names** — the navigation shows each translation's book names in
  its own language (Genèse, בְּרֵאשִׁית, Κατά Ιωάννην, Første Mosebok, …).
- **Right-to-left** rendering for Hebrew.
- **Command box** — type a reference (`John 3:16`, `1 Cor 13:4-7`, `Ps 23`) to jump
  there, or any other text to run a full-text search.
- **Full-text search** (SQLite FTS5), ranked, with match highlighting. Greek search
  is accent-insensitive.
- **Dark / light** themes.

## Translations & data sources

The five bundled translations:

| Code | Translation | Language | Tagged | Source / notes |
|------|-------------|----------|:------:|----------------|
| KJV  | King James Version | English | ✓ | [bolls.life](https://bolls.life/) KJV (1769) with Strong's numbers — public domain; clickable reverse interlinear |
| ULT  | unfoldingWord Literal Text | English | ✓ | [unfoldingWord ULT](https://git.door43.org/unfoldingWord/en_ult) — every word aligned to the original Hebrew/Greek with Strong's (CC BY-SA); powers the reverse interlinear |
| LSG  | Louis Segond (1910) | French | ✓ | public-domain Segond + a Strong's correspondence **built by statistical word-alignment** (`tools/build_lsg_strongs.py`); clickable reverse interlinear |
| WLC  | Westminster Leningrad Codex | Hebrew (OT) | ✓ | [OpenScriptures morphhb](https://github.com/openscriptures/morphhb) — the open Leningrad text BHS is based on, with per-word lemma + OSHM morphology |
| SBLGNT | SBL Greek New Testament | Greek (NT) | ✓ | [MorphGNT](https://github.com/morphgnt/sblgnt) — per-word parse code + lemma |
| LXX  | Septuagint | Greek (OT) | | getbible.net — includes deuterocanonical books |
| VUL  | Vulgata Clementina | Latin | | getbible.net — 66 canonical books only (deuterocanon dropped); shown as the Latin reference in the word-study panel |

Optional, **user-provided** translations (copyrighted — supplied by the user from
licensed EPUBs, **not committed** to the repo). Place the EPUBs and run
`tools/epub2json.py` to generate `data/<slug>.json`, then optionally
`tools/build_strongs.py` to add a Strong's correspondence (making them clickable
reverse interlinears). The importer includes each if present and skips it if not:

| Code | Translation | Language | From |
|------|-------------|----------|------|
| ESV  | English Standard Version | English | `ESV Bible - Crossway.epub` |
| BFC  | Bible en français courant | French | `…français courant…Collectif.epub` (deuterocanon + notes stripped) |
| NB   | Bibelen 2011 (Bokmål) | Norwegian | `Bibelen - bokmal - Bibelselskapet.epub` |

All three also get a **built Strong's correspondence** (`tools/build_strongs.py`),
so they're clickable reverse interlinears when present.

"Tagged" translations carry word-level data: WLC/SBLGNT have full morphology;
ULT carries each English word's Strong's number (normalised to match WLC/SBLGNT,
so an English click resolves to the actual original word in the verse).
Definitions come from the OpenScriptures **Strong's lexicons** (Hebrew XML + Greek
JSON). Greek lemmas are bridged to Strong's numbers by accent-insensitive matching
(≈98% of NT words resolve). Hebrew lemmas are Strong's numbers directly.

Book numbering is aligned across translations using a canonical book id
(1–39 OT, 40–66 NT), so parallel view and cross-translation word lookup line up.

## Installing

Prebuilt packages of **LeVerbe** are attached to each
[GitHub release](https://github.com/rmpr/logos-rs/releases). Both include the
complete Bible database, so the app works offline from the first launch.

- **Ubuntu 26.04 (Resolute Raccoon)**: `sudo apt install ./leverbe_<version>_amd64.deb`,
  then start **LeVerbe** from the app grid or run `leverbe`.
- **Windows (x64)**: run `LeVerbe-<version>-windows-x64-setup.exe` (installs per
  user, no admin rights needed), or unzip the portable
  `LeVerbe-<version>-windows-x64.zip` and run `LeVerbe.exe`.

Releases are built by `.github/workflows/release.yml`: push a tag `vX.Y.Z` to
publish one; running the workflow by hand only uploads the packages as build
artifacts. The packaging files live under `packaging/`.

## Building & running

Requires a recent stable Rust toolchain.

```sh
cargo run --bin logos-rs
```

That's it. On first launch the app builds the SQLite database
(`data/bible.db`, ~185k verses, ~1.2M tagged words) from the source texts
**vendored in the repo** (compressed under `data/`, ~24 MB) — so it works
**fully offline** and takes about 10 seconds, once. The database itself is
generated, not committed.

- Rebuild the database explicitly: `cargo run --bin import`.
- Point the app at a prebuilt database: set `$LOGOS_DB=/path/to/bible.db`.
- The importer reads each source from its committed `.gz`; if a raw (un-gzipped)
  copy is present it uses that, and only downloads from the network if a source
  is missing entirely.

## Architecture

```
src/
  canon.rs      Canonical 66-book table + abbreviations (reference parsing)
  reference.rs  Forgiving Scripture-reference parser ("1 Cor 13:4-7")
  morph.rs      Morphology decoders: OSHM (Hebrew) + MorphGNT (Greek) → prose
  bible.rs      Read-only data model over SQLite (verses, words, lexicon, FTS)
  importer.rs   Builds bible.db from getbible/morphhb/MorphGNT/ULT + Strong's lexicons
  main.rs       Slint UI wiring (state, callbacks, word wrapping)
ui/
  app.slint     UI: theme, nav, reading columns, search, parallel, word study
```

Database tables: `translations`, `books`, `verses` (+ `verses_fts`),
`words` (per-word lemma/Strong's/morphology), `lexicon` (Strong's definitions).

## Known limitations / TODO

- **Some Strong's correspondences are statistical**: where no Strong's-tagged
  edition exists (Louis Segond, ESV), the correspondence is *built* by IBM-Model-1
  word alignment (translation ↔ the Strong's in each verse from WLC/SBLGNT). Content
  words align well; some words are left untagged or occasionally mis-attached.
  Regenerate with `tools/build_strongs.py <db> <CODE> <out.json>`; for the
  EPUB-derived texts add `--source data/<slug>.json` (the output of
  `tools/epub2json.py`), since they enter the database only through this
  script's output.
- **LXX reverse lookup is verse-level**: no open, license-clean Septuagint is
  word-aligned to the Hebrew or carries Strong's, so an OT English click shows the
  precise Hebrew word but the LXX as the whole parallel verse (not the single Greek
  word). Word-level LXX alignment can follow if suitable data appears.
- **Hebrew search**: FTS5's `remove_diacritics` strips Greek accents but not Hebrew
  vowel points/cantillation, so unpointed Hebrew searches don't match yet. Planned
  fix: index a points-stripped copy of the Hebrew text (the `words` lemmas now make
  this straightforward).
- Word lines in tagged texts are wrapped using a character-width estimate (Slint has
  no flow layout), so they don't reflow on window resize.
- No cross-references, notes, or highlighting yet.
- The database is generated locally rather than bundled.


## Importing a Logos Personal Book Bible (LeVerbe Parallel)

Open **Import Bible (.docx)** in the left sidebar. Enter an absolute path to a
Logos-formatted DOCX, a new translation code (2–16 letters/digits/underscores),
a display name and a language code (e.g. `nb`, `en`, `grc`, `hbo`). Click
**Import translation**. The translation appears in the selectors immediately.

This recognizes per-verse milestones such as
`[[@BibleNO2011:gen 1:1]] 1 {{field-on:bible}}...{{field-off:bible}}`.
The label after `Bible` is treated as a reference namespace, **not** proof of
translation name or copyright status. Chapter headings are ignored. The import
is rejected if there are duplicate, invalid or empty recognized milestones.
Partial Bibles are supported; imported texts do not have morphological tagging.

The first import creates a private copy of the installed Bible database in the
user's LeVerbe Parallel data directory. Subsequent imports add translations to
that copy, surviving reinstallations and package upgrades without changing the
original LeVerbe. Note that new *bundled* translations from later updates will
not automatically appear in the private copy; a future migration tool can
synchronize them. Only import texts you have the right to use. DOCX text never
leaves your computer. This feature currently depends on `unzip` on Linux.

An equivalent command-line import (included in the Linux package) is:

```sh
leverbe-import-docx "/home/user/Documents/bible.docx" MYCODE "My translation" nb
```

Use **Manage translations** to hide/show translations in the dropdown menus.
Currently selected translations cannot be hidden.


### Automatic connections for imported DOCX translations

New imports automatically receive **unreviewed word-alignment suggestions**.
Every word becomes clickable; suggested connections open the matching Greek or
Hebrew occurrences from that same verse. Words without reliable evidence remain
unlinked. Bundled editorial tags and existing imports are unchanged.

Offline mode uses same-language tagged reference words, English lexicon glosses,
and repeated verse-level co-occurrence. Its coverage and accuracy depend on the
translation and document size. Small Norwegian samples often provide too little
evidence; importing a full translation helps. These are suggestions, not an
expert-reviewed reverse interlinear. Verse-number differences can reduce coverage.

Optional AI assistance is off by default. In **Import Bible**, select it, enter an
OpenAI model supporting Structured Outputs, and set the maximum number of verses
(default 20, maximum 1000). The app sends those verses' translated tokens and
original words/definitions to OpenAI; API usage is billed separately. It never
uploads the DOCX file or calls AI in offline mode. The request uses `store: false`.
Configure `OPENAI_API_KEY` in the app's launch environment; the app does not write
the key to its database or settings. Do not put keys in shared scripts or documents.
The model's confidence score is not a guarantee of accuracy. All generated links
remain marked as unreviewed, including AI links.

CLI offline import uses the same automatic process. To enable AI explicitly, add
`--ai` and configure `OPENAI_API_KEY`, `LEVERBE_ALIGNMENT_MODEL`, and optionally
`LEVERBE_AI_MAX_VERSES`. If AI fails, the import keeps offline suggestions and
reports the interruption. Imports run in a database transaction; keep the app
open until the completion message. AI mode processes at most the selected limit
of eligible verses; the rest retain offline suggestions. Unusually long verses
are kept local. Existing imported translations are not silently reprocessed.
