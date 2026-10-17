import json,tempfile,unittest,xml.etree.ElementTree as ET
from pathlib import Path
from zipfile import ZipFile
from prepare_docx_alignment import extract,embed
class PreparedDocument(unittest.TestCase):
 def test_numeric_text_and_visible_parts_are_preserved(self):
  with tempfile.TemporaryDirectory() as d:
   source=Path(d)/'source.docx';out=Path(d)/'prepared.docx'
   doc=b'<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>[[@Bible:Gen 1:1]] 1 {{field-on:bible}}72 cattle{{field-off:bible}}</w:t></w:r></w:p></w:body></w:document>'
   with ZipFile(source,'w') as z:
    z.writestr('word/document.xml',doc)
    z.writestr('word/_rels/document.xml.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>')
    z.writestr('[Content_Types].xml','<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"></Types>')
   verses=extract(source);self.assertEqual(verses[0]['tokens'],['72','cattle'])
   payload=dict(format='leverbe-alignment',version=1,language='en',verses=verses);embed(source,out,payload)
   with ZipFile(out) as z:
    self.assertEqual(z.read('word/document.xml'),doc)
    self.assertEqual(json.loads(ET.fromstring(z.read('customXml/leverbeAlignment.xml')).text),payload)
    self.assertEqual(len(ET.fromstring(z.read('word/_rels/document.xml.rels'))),1)
    self.assertEqual(len(ET.fromstring(z.read('[Content_Types].xml'))),1)
if __name__=='__main__':unittest.main()
