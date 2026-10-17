"""Smoke-test the installed package with isolated personal data."""
import hashlib
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
from zipfile import ZipFile


def digest(path):
    return hashlib.file_digest(path.open("rb"), "sha256").hexdigest()


package = "leverbe-parallel"
assert subprocess.check_output(
    ["dpkg-query", "-W", "-f=${Package}", package], text=True
) == package
paths = subprocess.check_output(["dpkg", "-L", package], text=True).splitlines()
assert "/usr/bin/leverbe-parallel" in paths
assert "/usr/bin/leverbe-import-docx" in paths
assert "/usr/share/applications/leverbe-parallel.desktop" in paths
assert "/usr/bin/leverbe" not in paths
assert "/usr/share/applications/leverbe.desktop" not in paths
assert not any(p == "/usr/share/leverbe" or p.startswith("/usr/share/leverbe/") for p in paths)
assert "/usr/share/leverbe-parallel/study-library.db" in paths
with sqlite3.connect("file:/usr/share/leverbe-parallel/study-library.db?mode=ro",uri=True) as library:
    assert library.execute("SELECT count(*) FROM works").fetchone()[0] >= 8
    assert library.execute("SELECT count(*) FROM tsk_refs").fetchone()[0] > 200000
base = Path("/usr/share/leverbe-parallel/bible.db")
before = digest(base)
with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    docx = tmp / "test.docx"
    with ZipFile(docx, "w") as archive:
        archive.writestr("word/document.xml", '''<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>[[@Bible:Gen</w:t></w:r><w:r><w:t>esis 1:1]] 1 {{field-on:bible}}Prøvetekst ἀγάπη ברא {{field-off:bible}}</w:t></w:r></w:p>
</w:body></w:document>''')
    env = dict(os.environ, XDG_DATA_HOME=str(tmp / "data"))
    cmd = ["/usr/bin/leverbe-import-docx", str(docx), "CITEST", "CI test", "nb"]
    subprocess.run(cmd, env=env, check=True)
    personal = tmp / "data/leverbe-parallel/personal-bible.db"
    assert personal.is_file(), personal
    with sqlite3.connect(personal) as db:
        assert db.execute("PRAGMA integrity_check").fetchone() == ("ok",)
        assert db.execute('''SELECT book, chapter, verse, text FROM verses
            JOIN translations ON translation_id=translations.id
            WHERE code='CITEST' ''').fetchall() == [(1, 1, 1, "Prøvetekst ἀγάπη ברא")]
        assert db.execute("SELECT count(*) FROM verses_fts WHERE verses_fts MATCH 'Prøvetekst'").fetchone()[0] > 0
    # Automatic offline import must connect a known English word to an
    # original-language occurrence, never to a lexicon entry absent in verse.
    english = tmp / "english.docx"
    with ZipFile(english, "w") as archive:
        archive.writestr("word/document.xml", '''<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>[[@Bible:Genesis 1:1]] God mysteryword</w:t></w:r></w:p></w:body></w:document>''')
    subprocess.run(["/usr/bin/leverbe-import-docx", str(english), "CIALIGN", "Alignment test", "en"], env=env, check=True)
    with sqlite3.connect(personal) as db:
        row = db.execute("SELECT w.strongs FROM words w JOIN translations t ON t.id=w.translation_id WHERE t.code='CIALIGN' AND w.position=0").fetchone()
        assert row == ("H430",), row
        assert db.execute("SELECT count(*) FROM personal_word_links p JOIN translations t ON t.id=p.translation_id WHERE t.code='CIALIGN'").fetchone()[0] > 0
        assert db.execute("SELECT w.strongs FROM words w JOIN translations t ON t.id=w.translation_id WHERE t.code='CIALIGN' AND w.position=1").fetchone() == (None,)
    # Prepared links are portable, source-specific and never regenerated on import.
    import json
    import xml.etree.ElementTree as ET
    prepared = tmp / "prepared.docx"
    with sqlite3.connect(base) as db:
        position,text,strongs,morph=db.execute("SELECT w.position,w.text,w.strongs,w.morph FROM words w JOIN translations t ON t.id=w.translation_id WHERE t.code='WLC' AND w.book=1 AND w.chapter=1 AND w.verse=1 AND w.strongs='H430'").fetchone()
    payload={"format":"leverbe-alignment","version":1,"language":"nb","verses":[{"book":1,"chapter":1,"verse":1,"tokens":["72","Gud","mysteryword"],"source":"WLC","issue":"Source wording preserved; sample warning.","links":[{"target":1,"sources":[{"position":position,"text":text,"strongs":strongs,"morph":morph}],"reviewed":True,"method":"context-review"}]}]}
    def write_prepared(payload):
        node=ET.Element('{urn:leverbe:alignment:1}alignment');node.text=json.dumps(payload,ensure_ascii=False)
        with ZipFile(prepared,'w') as archive:
            archive.writestr('word/document.xml','<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>[[@Bible:Genesis 1:1]] 1 {{field-on:bible}}72 Gud mysteryword{{field-off:bible}}</w:t></w:r></w:p></w:body></w:document>')
            archive.writestr('customXml/leverbeAlignment.xml',ET.tostring(node,encoding='utf-8'))
    write_prepared(payload)
    subprocess.run(['/usr/bin/leverbe-import-docx',str(prepared),'CIPREP','Prepared test','nb'],env=env,check=True)
    with sqlite3.connect(personal) as db:
        assert db.execute("SELECT text FROM verses v JOIN translations t ON t.id=v.translation_id WHERE t.code='CIPREP'").fetchone()==('72 Gud mysteryword',)
        assert db.execute("SELECT w.strongs FROM words w JOIN translations t ON t.id=w.translation_id WHERE t.code='CIPREP' ORDER BY position").fetchall()==[(None,),('H430',),(None,)]
        assert db.execute("SELECT p.method FROM personal_word_links p JOIN translations t ON t.id=p.translation_id WHERE t.code='CIPREP'").fetchall()==[('prepared-reviewed',)]
        assert db.execute("SELECT n.note FROM personal_alignment_notes n JOIN translations t ON t.id=n.translation_id WHERE t.code='CIPREP'").fetchone()[0].startswith('Source wording preserved')
    # Editing a target or using a different source edition must fail atomically.
    stable=digest(personal);payload['verses'][0]['tokens'][0]='changed';write_prepared(payload)
    assert subprocess.run(['/usr/bin/leverbe-import-docx',str(prepared),'CIBAD1','Invalid target','nb'],env=env).returncode!=0
    assert digest(personal)==stable
    payload['verses'][0]['tokens'][0]='72';payload['verses'][0]['links'][0]['sources'][0]['text']='different edition';write_prepared(payload)
    assert subprocess.run(['/usr/bin/leverbe-import-docx',str(prepared),'CIBAD2','Invalid source','nb'],env=env).returncode!=0
    assert digest(personal)==stable
    # Duplicate imports must fail without changing the database.
    personal_before = digest(personal)
    assert subprocess.run(cmd, env=env).returncode != 0
    assert digest(personal) == personal_before
assert digest(base) == before, "Packaged database was modified"
print("Installed package, Unicode DOCX import, FTS, duplicate rejection and database isolation passed")
