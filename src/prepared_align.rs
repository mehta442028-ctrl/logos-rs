//! Portable, position-specific links embedded in a DOCX. All data is untrusted:
//! validate exact target tokens and source word snapshots inside the import transaction.
use std::{collections::{BTreeMap,HashSet},path::Path,process::Command};
use rusqlite::{Connection,params};
use serde::Deserialize;
type Error=Box<dyn std::error::Error>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload { format:String, version:u32, language:String, pub verses:Vec<Verse> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verse { book:i64,chapter:i64,verse:i64,tokens:Vec<String>,source:String,links:Vec<Link>,issue:String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Link { target:usize,sources:Vec<Source>,reviewed:bool,method:String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source { position:i64,text:String,strongs:String,morph:String }
const PART:&str="customXml/leverbeAlignment.xml";
pub fn read(file:&Path,verses:&BTreeMap<(i64,i64,i64),String>,lang:&str)->Result<Option<Payload>,Error> {
    let list=Command::new("unzip").args(["-Z1","--"]).arg(file).output()?;
    if !list.status.success(){return Err("Cannot inspect DOCX parts".into());}
    let names=String::from_utf8(list.stdout)?;
    let count=names.lines().filter(|s|*s==PART).count();
    if count==0{return Ok(None)}
    if count!=1{return Err("Duplicate embedded alignment parts".into())}
    let data=Command::new("unzip").args(["-p","--"]).arg(file).arg(PART).output()?;
    if !data.status.success() || data.stdout.len()>100_000_000{return Err("Cannot read embedded alignment (100 MB limit)".into())}
    let xml=String::from_utf8(data.stdout)?;
    let doc=roxmltree::Document::parse(&xml)?;
    let root=doc.root_element();
    if !root.has_tag_name(("urn:leverbe:alignment:1","alignment")){return Err("Unknown alignment XML format".into())}
    let payload:Payload=serde_json::from_str(root.text().ok_or("Missing alignment JSON")?)?;
    validate(&payload,verses,lang)?;
    Ok(Some(payload))
}
fn validate(payload:&Payload,verses:&BTreeMap<(i64,i64,i64),String>,lang:&str)->Result<(),Error>{
    if payload.format!="leverbe-alignment" || payload.version!=1{return Err("Unsupported prepared alignment version".into())}
    if payload.language!=lang{return Err("Prepared alignment language does not match import language".into())}
    if payload.verses.len()!=verses.len(){return Err("Prepared alignment verse coverage does not match DOCX".into())}
    let mut seen=HashSet::new();
    for v in &payload.verses {
        let key=(v.book,v.chapter,v.verse);
        if !seen.insert(key){return Err("Duplicate prepared verse".into())}
        let actual=verses.get(&key).ok_or("Prepared reference is absent from DOCX")?;
        if actual.split_whitespace().collect::<Vec<_>>()!=v.tokens.iter().map(String::as_str).collect::<Vec<_>>() {
            return Err(format!("Prepared words do not match DOCX at {} {}:{}; regenerate links after editing text",v.book,v.chapter,v.verse).into())
        }
        if v.source!=if v.book<40{"WLC"}else{"SBLGNT"}{return Err("Prepared source is not the expected original-language edition".into())}
        if v.issue.len()>4000{return Err("Alignment issue exceeds text limit".into())}
        let mut targets=HashSet::new();
        for l in &v.links {
            if l.target>=v.tokens.len() || !targets.insert(l.target) || l.sources.is_empty() || l.sources.len()>30{return Err("Invalid or duplicate prepared target position".into())}
            if !["context-review","lexical-suggestion","corpus-suggestion"].contains(&l.method.as_str()) || (l.reviewed && l.method!="context-review") {return Err("Unknown prepared alignment method".into())}
            let mut sources=HashSet::new();
            if l.sources.iter().any(|s|s.position<0 || !sources.insert(s.position)){return Err("Invalid or duplicate prepared source position".into())}
        }
    }
    Ok(())
}
pub fn apply(conn:&Connection,tid:i64,payload:&Payload)->Result<crate::auto_align::Report,Error>{
    conn.execute_batch("CREATE TABLE IF NOT EXISTS personal_alignment_imports(translation_id INTEGER PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS personal_word_links(translation_id INTEGER NOT NULL,book INTEGER NOT NULL,chapter INTEGER NOT NULL,verse INTEGER NOT NULL,target_position INTEGER NOT NULL,source_translation_id INTEGER NOT NULL,source_position INTEGER NOT NULL,score REAL NOT NULL,method TEXT NOT NULL,PRIMARY KEY(translation_id,book,chapter,verse,target_position,source_translation_id,source_position));
        CREATE TABLE IF NOT EXISTS personal_alignment_notes(translation_id INTEGER,book INTEGER,chapter INTEGER,verse INTEGER,note TEXT,PRIMARY KEY(translation_id,book,chapter,verse));")?;
    let mut report=crate::auto_align::Report { verses:payload.verses.len(),prepared:true,..Default::default() };
    for v in &payload.verses {
        let sid:i64=conn.query_row("SELECT id FROM translations WHERE code=?1",[&v.source],|r|r.get(0))?;
        if !v.issue.is_empty(){report.flagged+=1;conn.execute("INSERT INTO personal_alignment_notes VALUES(?1,?2,?3,?4,?5)",params![tid,v.book,v.chapter,v.verse,v.issue])?;}
        let mut by_target=BTreeMap::new();
        for l in &v.links {
            for s in &l.sources {
                let actual:(String,String,String)=conn.query_row("SELECT text,COALESCE(strongs,''),COALESCE(morph,'') FROM words WHERE translation_id=?1 AND book=?2 AND chapter=?3 AND verse=?4 AND position=?5",params![sid,v.book,v.chapter,v.verse,s.position],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                if actual!=(s.text.clone(),s.strongs.clone(),s.morph.clone()){return Err(format!("Original-language edition differs at {} {}:{} word {}; no import saved",v.book,v.chapter,v.verse,s.position).into())}
                conn.execute("INSERT INTO personal_word_links VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![tid,v.book,v.chapter,v.verse,l.target as i64,sid,s.position,if l.reviewed{1.0}else{0.0},if l.reviewed{"prepared-reviewed"}else{"prepared-suggestion"}])?;
            }
            by_target.insert(l.target,l.sources[0].strongs.as_str());report.matched+=1;if l.reviewed{report.reviewed+=1;}
        }
        for (i,text) in v.tokens.iter().enumerate(){
            conn.execute("INSERT INTO words(translation_id,book,chapter,verse,position,text,lemma,strongs,morph,parsing) VALUES(?1,?2,?3,?4,?5,?6,'',?7,'','')",params![tid,v.book,v.chapter,v.verse,i as i64,text,by_target.get(&i).copied().filter(|s|!s.is_empty())])?;report.words+=1;
        }
    }
    conn.execute("UPDATE translations SET tagged=1 WHERE id=?1",[tid])?;
    conn.execute("INSERT INTO personal_alignment_imports VALUES(?1)",[tid])?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn payload()->Payload {serde_json::from_value(serde_json::json!({"format":"leverbe-alignment","version":1,"language":"nb","verses":[{"book":1,"chapter":1,"verse":1,"tokens":["Gud"],"source":"WLC","links":[{"target":0,"sources":[{"position":0,"text":"אֱלֹהִים","strongs":"H430","morph":"HNcmpa"}],"reviewed":true,"method":"context-review"}],"issue":"Source has missing words"}]})).unwrap()}
    #[test]fn reject_stale_tokens_and_duplicate_targets(){let mut p=payload();let verses=BTreeMap::from([((1,1,1),"Gud".into())]);validate(&p,&verses,"nb").unwrap();p.verses[0].tokens[0]="modified".into();assert!(validate(&p,&verses,"nb").is_err());}
    #[test]fn source_snapshot_must_match_and_links_are_persisted(){
        let db=Connection::open_in_memory().unwrap();db.execute_batch("CREATE TABLE translations(id INTEGER PRIMARY KEY,code TEXT,tagged INTEGER);CREATE TABLE words(translation_id INTEGER,book INTEGER,chapter INTEGER,verse INTEGER,position INTEGER,text TEXT,lemma TEXT,strongs TEXT,morph TEXT,parsing TEXT);INSERT INTO translations VALUES(1,'WLC',1),(2,'TEST',0);INSERT INTO words VALUES(1,1,1,1,0,'אֱלֹהִים','','H430','HNcmpa','');").unwrap();
        let p=payload();let report=apply(&db,2,&p).unwrap();assert_eq!(report.reviewed,1);assert_eq!(report.flagged,1);
        assert_eq!(db.query_row("SELECT method FROM personal_word_links",[],|r|r.get::<_,String>(0)).unwrap(),"prepared-reviewed");
        let mut bad=payload();bad.verses[0].links[0].sources[0].text="different".into();assert!(apply(&db,3,&bad).is_err());
    }
}
