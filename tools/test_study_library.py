#!/usr/bin/env python3
import sys,sqlite3,unittest
from build_study_library import refs,verse_keys,ROOT
class References(unittest.TestCase):
 def test_compact(self):
  self.assertEqual([(b,c,v,ec,ev) for b,c,v,ec,ev,_ in refs('Joh 1:1-3; 3:16,18; Heb 11:3')],[(43,1,1,1,3),(43,3,16,3,16),(43,3,18,3,18),(58,11,3,11,3)])
 def test_invalid(self):
  self.assertEqual(list(refs('Ge 1:99; Bogus 3:1')),[])
 def test_versification(self):
  self.assertEqual(len(verse_keys('ot')),24115);self.assertEqual(len(verse_keys('nt')),8246)
 def test_database(self):
  if '--database' not in sys.argv:self.skipTest('build database first')
  db=sqlite3.connect(ROOT/'data/study-library.db')
  self.assertEqual(db.execute('PRAGMA integrity_check').fetchone()[0],'ok')
  self.assertGreater(db.execute('SELECT count(*) FROM tsk_entries').fetchone()[0],28000)
  rows=db.execute('SELECT target_book,target_chapter,target_verse FROM tsk_refs WHERE book=1 AND chapter=1 AND verse=1').fetchall()
  self.assertIn((43,1,1),rows);self.assertIn((58,11,3),rows)
  for title,minimum in [('Matthew Henry',1000),('John Calvin',500),('Matthew Poole',100),('J. C. Ryle',100),('John Owen',100)]:
   count=db.execute('SELECT count(*) FROM sections s JOIN works w ON w.id=s.work_id WHERE w.title LIKE ?',('%'+title+'%',)).fetchone()[0];self.assertGreater(count,minimum,title)
  self.assertTrue(db.execute("SELECT 1 FROM sections s JOIN works w ON w.id=s.work_id WHERE w.title LIKE 'Matthew Henry%' AND s.book=66 AND s.chapter=22").fetchone())
if __name__=='__main__':unittest.main(argv=[sys.argv[0]])
