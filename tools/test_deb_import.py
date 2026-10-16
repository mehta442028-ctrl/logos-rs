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
base = Path("/usr/share/leverbe-parallel/bible.db")
before = digest(base)
with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    docx = tmp / "test.docx"
    with ZipFile(docx, "w") as archive:
        archive.writestr("word/document.xml", '''<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>[[@Bible:Gen</w:t></w:r><w:r><w:t>esis 1:1]] {{field-on:bible}}1 Prøvetekst ἀγάπη ברא {{field-off:bible}}</w:t></w:r></w:p>
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
    # Duplicate imports must fail without changing the database.
    personal_before = digest(personal)
    assert subprocess.run(cmd, env=env).returncode != 0
    assert digest(personal) == personal_before
assert digest(base) == before, "Packaged database was modified"
print("Installed package, Unicode DOCX import, FTS, duplicate rejection and database isolation passed")
