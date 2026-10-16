#!/usr/bin/env python3
"""Build the bundled offline library from checksum-pinned public-domain sources.
Python standard library only. Downloads happen at package build time, never in
normal application use. Preserve source attribution and scan/OCR notices.
"""
from pathlib import Path
import collections, gzip, hashlib, html, json, re, sqlite3, struct, sys, urllib.request, zipfile, zlib
from html.parser import HTMLParser
ROOT=Path(__file__).resolve().parents[1]
BOOKS=['Genesis','Exodus','Leviticus','Numbers','Deuteronomy','Joshua','Judges','Ruth','1 Samuel','2 Samuel','1 Kings','2 Kings','1 Chronicles','2 Chronicles','Ezra','Nehemiah','Esther','Job','Psalms','Proverbs','Ecclesiastes','Song of Solomon','Isaiah','Jeremiah','Lamentations','Ezekiel','Daniel','Hosea','Joel','Amos','Obadiah','Jonah','Micah','Nahum','Habakkuk','Zephaniah','Haggai','Zechariah','Malachi','Matthew','Mark','Luke','John','Acts','Romans','1 Corinthians','2 Corinthians','Galatians','Ephesians','Philippians','Colossians','1 Thessalonians','2 Thessalonians','1 Timothy','2 Timothy','Titus','Philemon','Hebrews','James','1 Peter','2 Peter','1 John','2 John','3 John','Jude','Revelation']
SHORT=['Ge','Ex','Le','Nu','De','Jos','Jud','Ru','1Sa','2Sa','1Ki','2Ki','1Ch','2Ch','Ezr','Ne','Es','Job','Ps','Pr','Ec','So','Isa','Jer','La','Eze','Da','Ho','Joe','Am','Ob','Jon','Mic','Na','Hab','Zep','Hag','Zec','Mal','Mt','Mr','Lu','Joh','Ac','Ro','1Co','2Co','Ga','Eph','Php','Col','1Th','2Th','1Ti','2Ti','Tit','Phm','Heb','Jas','1Pe','2Pe','1Jo','2Jo','3Jo','Jude','Re']
ALIASES={re.sub('[^a-z0-9]','',s.lower()):i+1 for i,(a,b) in enumerate(zip(BOOKS,SHORT)) for s in (a,b)}
ALIASES.update({'gen':1,'exod':2,'lev':3,'num':4,'deut':5,'josh':6,'jdg':7,'ruth':8,'esth':17,'psalm':19,'prov':20,'eccl':21,'song':22,'ezek':26,'dan':27,'hos':28,'joel':29,'amos':30,'obad':31,'jonah':32,'nah':34,'zeph':36,'matt':40,'mk':41,'mark':41,'luk':42,'jn':43,'act':44,'rom':45,'gal':48,'phil':50,'phile':57,'james':59,'jud':7,'rev':66})
COUNTS={}
for v in json.load(gzip.open(ROOT/'data/kjv_strongs.json.gz')):
    if int(v['book'])>66 or (int(v['book'])==17 and int(v['chapter'])>10):continue
    key=(int(v['book']),int(v['chapter']));COUNTS[key]=max(COUNTS.get(key,0),int(v['verse']))
COUNTS[17,10]=3 # KJV Esther ends at 10:3; the source also includes Greek additions.
class Plain(HTMLParser):
    def __init__(self):super().__init__(convert_charrefs=True);self.parts=[]
    def handle_starttag(self,tag,attrs):
        a=dict(attrs)
        if tag in ['p','br','lb','title','h1','h2','h3','h4'] or (tag=='div' and not a.get('eid')):self.parts.append('\n\n')
    def handle_endtag(self,tag):
        if tag in ['p','title','h1','h2','h3','h4']:self.parts.append('\n\n')
    def handle_data(self,data):self.parts.append(data)
def plaintext(raw):
    p=Plain();p.feed(raw);return '\n\n'.join(' '.join(x.split()) for x in ''.join(p.parts).split('\n\n') if x.strip())
def refs(text,default_book=None,default_chapter=None):
    """TSK's compact references carry book/chapter across semicolons/commas."""
    book,chapter=default_book,default_chapter
    for group in text.replace('–','-').replace('—','-').split(';'):
        group=group.strip().strip('.')
        m=re.match(r'^([1-3]?\s*[A-Za-z]+)\.?\s+(.*)$',group)
        explicit_book=False
        if m:
            book=ALIASES.get(re.sub('[^a-z0-9]','',m[1].lower()));group=m[2];explicit_book=True
            if not book:continue
        for part in group.split(','):
            part=part.strip().strip('.')
            m=re.fullmatch(r'(?:(\d+):)?(\d+)(?:-(?:(\d+):)?(\d+))?',part)
            if not m or not book:continue
            ch,start,endch,end=m.groups()
            if ch:chapter=int(ch)
            elif explicit_book:
                chapter=int(start);start='1';end=str(COUNTS.get((book,chapter),0));endch=str(chapter)
            if not chapter:continue
            v=int(start);ec=int(endch) if endch else chapter;ev=int(end) if end else v
            if v<1 or ev<1 or (ec,ev)<(chapter,v):continue
            if v>COUNTS.get((book,chapter),0) or ev>COUNTS.get((book,ec),0):continue
            label=f'{BOOKS[book-1]} {chapter}:{v}'+(f'-{ec}:{ev}' if ec!=chapter else f'-{ev}' if ev!=v else '')
            yield book,chapter,v,ec,ev,label
            explicit_book=False

def download(source,cache):
    p=cache/source['file'];p.parent.mkdir(parents=True,exist_ok=True)
    if not p.exists():
        errors=[]
        for url in source.get('mirrors',[])+[source['url']]:
            for attempt in range(2):
                try:
                    print('Downloading',source['file'],'from',urllib.parse.urlsplit(url).netloc,'attempt',attempt+1,flush=True)
                    request=urllib.request.Request(url,headers={'User-Agent':'LeVerbe-Parallel-library-builder/1.0'})
                    with urllib.request.urlopen(request,timeout=25) as r, p.with_suffix(p.suffix+'.part').open('wb') as f:
                        while block:=r.read(1024*1024):f.write(block)
                    if hashlib.sha256(p.with_suffix(p.suffix+'.part').read_bytes()).hexdigest()!=source['sha256']:
                        raise RuntimeError('Checksum mismatch: '+source['file'])
                    p.with_suffix(p.suffix+'.part').replace(p)
                    break
                except (OSError,urllib.error.URLError) as error:
                    errors.append(str(error));print('Source connection failed:',source['file'],str(error),flush=True)
            if p.exists():break
        if not p.exists():raise RuntimeError('All public source mirrors failed for '+source['file']+': '+'; '.join(errors))
    if hashlib.sha256(p.read_bytes()).hexdigest()!=source['sha256']:raise RuntimeError('Checksum mismatch: '+source['file'])
    return p

def verse_keys(testament):
    result=[None,None]
    for b in range(1 if testament=='ot' else 40,40 if testament=='ot' else 67):
        result.append((b,0,0))
        for c in range(1,max(ch for book,ch in COUNTS if book==b)+1):
            result.append((b,c,0));result.extend((b,c,v) for v in range(1,COUNTS[b,c]+1))
    return result

def sword_entries(path):
    z=zipfile.ZipFile(path);conf=z.read(next(n for n in z.namelist() if n.endswith('.conf'))).decode('utf8')
    if not re.search(r'^DistributionLicense=Public Domain\s*$',conf,re.M):raise RuntimeError('Source is not labelled Public Domain')
    data=re.search(r'^DataPath=(.*)$',conf,re.M)[1].strip().removeprefix('./').rstrip('/')+'/'
    encoding='utf8' if 'Encoding=UTF-8' in conf else 'latin1'
    drv=re.search(r'^ModDrv=(.*)$',conf,re.M)[1].strip();size=12 if drv.endswith('4') else 10
    blockprefix='c' if 'BlockType=CHAPTER' in conf else 'b'
    for testament in ['ot','nt']:
        prefix=data+testament+'.'+blockprefix
        if prefix+'zv' not in z.namelist():continue
        idx=z.read(prefix+'zv');blocks=z.read(prefix+'zs');raw=z.read(prefix+'zz');keys=verse_keys(testament)
        if len(idx)!=len(keys)*size:raise RuntimeError(f'Versification/index mismatch: {path} {testament}')
        cached_block=None;uncompressed=b''
        for i,key in enumerate(keys):
            block,offset,length=struct.unpack_from('<III' if size==12 else '<IIH',idx,i*size)
            if key is None or not length:continue
            if block!=cached_block:
                bo,bl,un=struct.unpack_from('<III',blocks,block*12);uncompressed=zlib.decompress(raw[bo:bo+bl]);cached_block=block
                if len(uncompressed)!=un:raise RuntimeError('Corrupt compressed source block')
            yield key,(testament,block,offset,length),uncompressed[offset:offset+length].decode(encoding,'replace')

def build(manifest,cache,destination):
    # Fetch independently so a slow archive connection cannot hold all other sources.
    import concurrent.futures
    sources=[s for w in manifest['works'] for s in w['sources']]
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:list(pool.map(lambda s:download(s,cache),sources))
    destination.unlink(missing_ok=True)
    db=sqlite3.connect(destination)
    db.executescript('''PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
        CREATE TABLE works(id INTEGER PRIMARY KEY,title TEXT,source TEXT,notice TEXT,sort INTEGER);
        CREATE TABLE sections(id INTEGER PRIMARY KEY,work_id INTEGER,title TEXT,book INTEGER,chapter INTEGER,ordinal INTEGER,body TEXT);
        CREATE INDEX sections_work ON sections(work_id,ordinal);
        CREATE TABLE tsk_entries(book INTEGER,chapter INTEGER,verse INTEGER,body TEXT,PRIMARY KEY(book,chapter,verse));
        CREATE TABLE tsk_refs(book INTEGER,chapter INTEGER,verse INTEGER,ordinal INTEGER,phrase TEXT,label TEXT,target_book INTEGER,target_chapter INTEGER,target_verse INTEGER,end_chapter INTEGER,end_verse INTEGER);
        CREATE INDEX tsk_ref_source ON tsk_refs(book,chapter,verse,ordinal);''')
    for sort,work in enumerate(manifest['works']):
        wid=sort+1
        if work['kind']=='tsk':
            source=work['sources'][0];path=download(source,cache)
            for (b,c,v),_,raw in sword_entries(path):
                if not c or not v:continue
                db.execute('INSERT INTO tsk_entries VALUES(?,?,?,?)',(b,c,v,plaintext(raw)))
                seen=set();ordinal=0
                for attrs,inner in re.findall(r'<scripRef\b([^>]*)>(.*?)</scripRef>',raw,re.I|re.S):
                    m=re.search(r'passage="([^"]+)"',attrs,re.I);text=html.unescape(m[1] if m else plaintext(inner))
                    for rb,rc,rv,ec,ev,label in refs(text,b,c):
                        key=(rb,rc,rv,ec,ev)
                        if key in seen:continue
                        seen.add(key);db.execute('INSERT INTO tsk_refs VALUES(?,?,?,?,?,?,?,?,?,?,?)',(b,c,v,ordinal,'',label,rb,rc,rv,ec,ev));ordinal+=1
            print('TSK entries',db.execute('SELECT count(*) FROM tsk_entries').fetchone()[0],flush=True);continue
        db.execute('INSERT INTO works VALUES(?,?,?,?,?)',(wid,work['title'],work['source'],work['notice'],sort))
        if work['kind']=='sword':
            path=download(work['sources'][0],cache);chapters=collections.OrderedDict();seen=collections.defaultdict(set)
            for (b,c,v),key,raw in sword_entries(path):
                if key in seen[b,c]:continue
                seen[b,c].add(key);text=plaintext(raw)
                if text:chapters.setdefault((b,c),[]).append((f'Verse {v}\n' if v else '')+text)
            for ordinal,((b,c),texts) in enumerate(chapters.items()):
                title=f'{BOOKS[b-1]} '+(str(c) if c else '— Introduction')
                db.execute('INSERT INTO sections(work_id,title,book,chapter,ordinal,body) VALUES(?,?,?,?,?,?)',(wid,title,b,c,ordinal,'\n\n'.join(texts)))
        else:
            ordinal=0
            for source in work['sources']:
                text=download(source,cache).read_text(encoding='utf8',errors='replace')
                paragraphs=[' '.join(p.split()) for p in re.split(r'\n\s*\n',text) if p.strip()]
                chunk=[];length=0;part=1
                for paragraph in paragraphs+['']:
                    if chunk and (length+len(paragraph)>16000 or not paragraph):
                        db.execute('INSERT INTO sections(work_id,title,book,chapter,ordinal,body) VALUES(?,?,?,?,?,?)',(wid,f"{source['label']} · Part {part}",0,part,ordinal,'\n\n'.join(chunk)));ordinal+=1;part+=1;chunk=[];length=0
                    if paragraph:chunk.append(paragraph);length+=len(paragraph)
        db.commit();print(work['title'],db.execute('SELECT count(*) FROM sections WHERE work_id=?',(wid,)).fetchone()[0],flush=True)
    db.commit();assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
    assert db.execute('SELECT count(*) FROM tsk_entries').fetchone()[0]>28000
    assert db.execute('SELECT count(*) FROM tsk_refs').fetchone()[0]>200000
    db.execute('VACUUM');db.close()
if __name__=='__main__':
    manifest=json.loads((ROOT/'data/study/sources.json').read_text())
    cache=Path(sys.argv[1]) if len(sys.argv)>1 else ROOT/'target/study-sources';cache.mkdir(parents=True,exist_ok=True)
    build(manifest,cache,ROOT/'data/study-library.db')
