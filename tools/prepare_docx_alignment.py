#!/usr/bin/env python3
"""Prepare private DOCX alignment metadata. Never uploads translation text.
Whitespace token indexes are zero-based. Original word snapshots prevent stale
links being silently applied against a changed original-language edition.
"""
from pathlib import Path
from zipfile import ZipFile,ZIP_DEFLATED
import argparse,collections,json,re,sqlite3,unicodedata,xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
W='http://schemas.openxmlformats.org/wordprocessingml/2006/main'
def normalize(s):return ''.join(c for c in unicodedata.normalize('NFKD',s.lower()) if c.isalnum())
STOP=set('og i på av en et den det de som til fra for med om at så er var har hadde skal skulle vil ville kan kunne han hun ham henne hans hennes sin sitt sine jeg meg min mitt mine du deg din ditt dine vi oss vår vårt våre dere deres dem seg ikke men eller når da dette disse denne blir ble bli vært være også alle alt hver hvem hva hvor hvorfor hvordan nå her der mer mest bare både mot over under mellom gjennom etter før uten inn ut opp ned'.split())
# Lexical evidence; these remain SUGGESTIONS, never context-reviewed by this script.
SEEDS={
 'gud':['H430','H410','G2316'],'guds':['H430','H410','G2316'],'jesus':['G2424'],
 'kristus':['G5547'],'kristi':['G5547'],'ånd':['H7307','G4151'],'ånden':['H7307','G4151'],
 'herren':['H3068','H136','G2962'],'herre':['H3068','H136','G2962'],
 'fader':['H1','G3962'],'far':['H1','G3962'],'faren':['H1','G3962'],
 'sønn':['H1121','G5207'],'sønnen':['H1121','G5207'],'sønnene':['H1121','G5207'],
 'datter':['H1323','G2364'],'døtre':['H1323','G2364'],
 'israel':['H3478','G2474'],'david':['H1732','G1138'],'moses':['H4872','G3475'],
 'abraham':['H85','G11'],'jakob':['H3290','G2384'],'josef':['H3130','G2501'],
 'paulus':['G3972'],'peter':['G4074'],'jerusalem':['H3389','G2419','G2414'],
 'egypt':['H4714','G125'],'isak':['H3327','G2464'],'israelittene':['H3478','G2474'],
 'brød':['H3899','G740'],'brødet':['H3899','G740'],'vann':['H4325','G5204'],
 'vannet':['H4325','G5204'],'ild':['H784','G4442'],'ilden':['H784','G4442'],
 'lys':['H216','G5457'],'lyset':['H216','G5457'],'mørke':['H2822','G4655'],
 'mørket':['H2822','G4655'],'jord':['H776','G1093'],'jorden':['H776','G1093'],
 'himmel':['H8064','G3772'],'himmelen':['H8064','G3772'],'himmelenes':['H8064','G3772'],
 'liv':['H2416','G2222'],'livet':['H2416','G2222'],'evig':['H5769','G166'],
 'død':['H4194','G2288'],'døden':['H4194','G2288'],'blod':['H1818','G129'],
 'blodet':['H1818','G129'],'konge':['H4428','G935'],'kongen':['H4428','G935'],
 'konger':['H4428','G935'],'prest':['H3548','G2409'],'presten':['H3548','G2409'],
 'prestene':['H3548','G2409'],'profet':['H5030','G4396'],'profetene':['H5030','G4396'],
 'engelen':['H4397','G32'],'engel':['H4397','G32'],'engler':['H4397','G32'],
 'hjerte':['H3820','H3824','G2588'],'hjertet':['H3820','H3824','G2588'],
 'hånd':['H3027','G5495'],'hender':['H3027','G5495'],'hånden':['H3027','G5495'],
 'øye':['H5869','G3788'],'øyne':['H5869','G3788'],'øynene':['H5869','G3788'],
 'øre':['H241','G3775'],'ører':['H241','G3775'],'ord':['H1697','G3056','G4487'],
 'ordet':['H1697','G3056','G4487'],'tro':['G4102'],'tror':['G4100'],'trodde':['G4100'],
 'kjærlighet':['G26'],'elsker':['H157','G25'],'elsket':['H157','G25'],
 'synd':['H2403','G266'],'synden':['H2403','G266'],'synder':['H2403','G266'],
 'lov':['H8451','G3551'],'loven':['H8451','G3551'],'fred':['H7965','G1515'],
 'nåde':['H2580','G5485'],'nåden':['H2580','G5485'],'sannhet':['H571','G225'],
 'sannheten':['H571','G225'],'legeme':['G4983'],'kropp':['G4983'],
 'verden':['H8398','G2889'],'rike':['H4438','H4467','G932'],'riket':['H4438','H4467','G932'],
 'begynnelsen':['H7225','G746'],'skapte':['H1254','G2936'],
 'frelser':['H3467','G4990'],'disipler':['G3101'],'disiplene':['G3101'],
 'hellig':['H6944','H6918','G40'],'hellige':['H6944','H6918','G40'],
 'visdom':['H2451','G4678'],'visdommen':['H2451','G4678'],
}
STOP={normalize(s) for s in STOP}
SEEDS={normalize(k):v for k,v in SEEDS.items()}
def extract(path):
    aliases={a:int(n) for n,raw in re.findall(r'BookInfo \{ nr: (\d+),.*?aliases: &\[(.*?)\]',(ROOT/'src/canon.rs').read_text()) for a in re.findall(r'"([^"]+)"',raw)}
    with ZipFile(path) as z:root=ET.fromstring(z.read('word/document.xml'))
    result=[];seen=set()
    for p in root.iter('{'+W+'}p'):
        text=''.join(t.text or '' for t in p.iter('{'+W+'}t'))
        m=re.search(r'\[\[@Bible[A-Za-z0-9_]*:\s*([A-Za-z0-9 ]+?)\s+(\d+):(\d+)\]\]',text)
        if not m:continue
        key=(aliases[normalize(m[1])],int(m[2]),int(m[3]));after=text[m.end():]
        start=re.search(r'\{\{field-on:bible\}\}',after,re.I)
        if start:body=re.split(r'\{\{field-off:bible\}\}',after[start.end():],flags=re.I)[0]
        else:body=re.sub(r'^\s*\d+\s+','',after)
        body=re.sub(r'\{\{field-(?:on|off):bible\}\}','',body,flags=re.I)
        if key in seen:raise ValueError('Duplicate verse '+str(key))
        seen.add(key);result.append(dict(book=key[0],chapter=key[1],verse=key[2],tokens=body.split(),source='WLC' if key[0]<40 else 'SBLGNT',links=[],issue=''))
    return result

def prepare(source,db,language,reviewed):
    verses=extract(source);conn=sqlite3.connect('file:'+str(db.resolve())+'?mode=ro',uri=True)
    originals=collections.defaultdict(list);counts=collections.Counter()
    for b,c,v,position,text,strongs,morph in conn.execute("SELECT w.book,w.chapter,w.verse,w.position,w.text,coalesce(w.strongs,''),coalesce(w.morph,'') FROM words w JOIN translations t ON t.id=w.translation_id WHERE (t.code='WLC' AND w.book<40) OR (t.code='SBLGNT' AND w.book BETWEEN 40 AND 66) ORDER BY w.book,w.chapter,w.verse,w.position"):
        originals[b,c,v].append(dict(position=position,text=text,strongs=strongs,morph=morph))
    for b,c,v in originals:counts[b,c]+=1
    target_counts=collections.Counter((v['book'],v['chapter']) for v in verses)
    mismatched={key for key,n in target_counts.items() if n!=counts[key]}
    # Different chapter boundaries can shift all subsequent references. Do not guess a mapping.
    mismatch_books={b for b,c in mismatched if b in (29,39)}
    tf=collections.Counter();sf=collections.Counter();joint=collections.defaultdict(collections.Counter)
    def eligible(v):return (v['book'],v['chapter']) not in mismatched and v['book'] not in mismatch_books
    for v in verses:
        if not eligible(v):continue
        ws=originals.get((v['book'],v['chapter'],v['verse']),[]);testament=v['source']
        tokens={normalize(t) for t in v['tokens'] if len(normalize(t))>2 and normalize(t) not in STOP}
        strongs={w['strongs'] for w in ws if w['strongs']}
        for strong in strongs:sf[testament,strong]+=1
        for t in tokens:
            tf[testament,t]+=1
            for strong in strongs:joint[testament,t][strong]+=1
    reviewed_count=0
    for v in verses:
        ref=(v['book'],v['chapter'],v['verse']);ws=originals.get(ref,[]);issues=[]
        if not eligible(v):issues.append('Verse/chapter numbering differs from the bundled original; no automatic links assigned here.')
        if not ws:issues.append('No original-language verse is available at this reference.')
        if v['tokens'] and v['tokens'][0][0] in ',;:!?':issues.append('Source text begins with punctuation; possible omitted opening words. Wording preserved.')
        if len(v['tokens'])<3:issues.append('Very short source verse; check for omissions. Wording preserved.')
        if v['book']==19 and v['chapter']==3 and v['verse']>=2:issues.append('Missing wording confirmed against the publisher’s 2011 Psalm 3. Absent words have not been restored.')
        if ref==(1,2,4):issues.append('Source omits Herren before Gud. Wording preserved.')
        unique=collections.defaultdict(list)
        for w in ws:
            if w['strongs']:unique[w['strongs']].append(w)
        if eligible(v):
            for i,t in enumerate(v['tokens']):
                token=normalize(t)
                if not token or token in STOP:continue
                scores=[]
                for strong,ow in unique.items():
                    if len(ow)!=1:continue
                    support=joint[v['source'],token][strong]
                    dice=2*support/max(1,tf[v['source'],token]+sf[v['source'],strong])
                    lexical=strong in SEEDS.get(token,[]) if language=='nb' else False
                    if lexical:scores.append((2.0,strong,'lexical-suggestion'))
                    elif len(token)>3 and support>=8 and dice>=0.72:scores.append((dice,strong,'corpus-suggestion'))
                scores.sort(reverse=True)
                if not scores or (len(scores)>1 and scores[0][0]<scores[1][0]*1.8):continue
                _,strong,method=scores[0];v['links'].append(dict(target=i,sources=unique[strong],reviewed=False,method=method))
        custom=reviewed.get(':'.join(map(str,ref)))
        if custom is not None:
            if custom['tokens']!=v['tokens']:raise ValueError('Reviewed target wording differs: '+str(ref))
            by_position={w['position']:w for w in ws};v['links']=[]
            for target,positions in custom['links']:
                v['links'].append(dict(target=target,sources=[by_position[n] for n in positions],reviewed=True,method='context-review'));reviewed_count+=1
            if custom.get('note'):issues.append(custom['note'])
        v['issue']=' '.join(issues)
    payload=dict(format='leverbe-alignment',version=1,language=language,verses=verses)
    summary=dict(verses=len(verses),words=sum(len(v['tokens']) for v in verses),linked_words=sum(len(v['links']) for v in verses),context_reviewed_words=reviewed_count,flagged_verses=sum(bool(v['issue']) for v in verses),numbering_excluded_chapters=sorted(mismatched),source_editions=['WLC','SBLGNT'],status='Automatic suggestions are unreviewed. Only explicit context-review entries are reviewed; unmatched words have no link. Known gaps are preserved; flags are not a complete list of source omissions.')
    return payload,summary

def embed(source,destination,payload):
    name='customXml/leverbeAlignment.xml';ns='http://schemas.openxmlformats.org/package/2006/relationships';ct='http://schemas.openxmlformats.org/package/2006/content-types'
    root=ET.Element('{urn:leverbe:alignment:1}alignment');root.text=json.dumps(payload,ensure_ascii=False,separators=(',',':'));xml=ET.tostring(root,encoding='utf-8',xml_declaration=True)
    with ZipFile(source) as src,ZipFile(destination,'w',ZIP_DEFLATED) as dst:
        rels=ET.fromstring(src.read('word/_rels/document.xml.rels'));ids={r.attrib['Id'] for r in rels};n=1
        while 'rIdLeVerbe'+str(n) in ids:n+=1
        ET.SubElement(rels,'{'+ns+'}Relationship',Id='rIdLeVerbe'+str(n),Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml',Target='../'+name)
        types=ET.fromstring(src.read('[Content_Types].xml'));ET.SubElement(types,'{'+ct+'}Override',PartName='/'+name,ContentType='application/xml')
        for info in src.infolist():
            if info.filename==name:continue
            data=src.read(info.filename)
            if info.filename=='word/_rels/document.xml.rels':data=ET.tostring(rels,encoding='utf-8',xml_declaration=True)
            if info.filename=='[Content_Types].xml':data=ET.tostring(types,encoding='utf-8',xml_declaration=True)
            dst.writestr(info,data)
        dst.writestr(name,xml)
    with ZipFile(source) as a,ZipFile(destination) as b:
        for n in a.namelist():
            if n not in ('word/_rels/document.xml.rels','[Content_Types].xml'):assert a.read(n)==b.read(n),n
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('source',type=Path);p.add_argument('output',type=Path);p.add_argument('--database',type=Path,required=True);p.add_argument('--language',default='nb');p.add_argument('--reviewed',type=Path);p.add_argument('--report',type=Path,required=True);a=p.parse_args()
    review=json.loads(a.reviewed.read_text()) if a.reviewed else {};payload,summary=prepare(a.source,a.database,a.language,review);embed(a.source,a.output,payload);a.report.write_text(json.dumps(summary,ensure_ascii=False,indent=2));print(json.dumps(summary,ensure_ascii=False))
