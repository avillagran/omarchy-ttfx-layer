//! Bounded general Unicode/style cache; full-key equality, original map owns slots.
const EMPTY:u64=u64::MAX;
pub const STYLE_MASK:u64=1|2|4|64;
#[derive(Clone,Copy)]struct Entry{key:u64,slot:u16}
pub struct SlotCache{entries:Box<[Entry;256]>,#[cfg(test)]pub hits:u64,#[cfg(test)]pub misses:u64}
impl SlotCache{
    pub fn new()->Self{Self{entries:Box::new([Entry{key:EMPTY,slot:0};256]),#[cfg(test)]hits:0,#[cfg(test)]misses:0}}
    pub fn key(ch:char,style:u64)->Option<u64>{(style&!STYLE_MASK==0).then_some(((ch as u64)<<8)|style)}
    pub fn index(key:u64)->usize{((key^(key>>8)^(key>>16)).wrapping_mul(0x9e3779b97f4a7c15)>>56)as usize}
    pub fn get(&mut self,ch:char,style:u64)->Option<u16>{
        let found=Self::key(ch,style).and_then(|key|{let e=self.entries[Self::index(key)];(e.key==key).then_some(e.slot)});
        #[cfg(test)]{if found.is_some(){self.hits+=1}else{self.misses+=1}}
        found
    }
    pub fn insert(&mut self,ch:char,style:u64,slot:u16){if slot==0{return}if let Some(key)=Self::key(ch,style){self.entries[Self::index(key)]=Entry{key,slot};}}
    pub fn clear(&mut self){self.entries.fill(Entry{key:EMPTY,slot:0});}
    pub fn payload_bytes()->usize{std::mem::size_of::<Entry>()*256}
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn full_key_and_style_guards_prevent_aliases(){let mut c=SlotCache::new();for style in 0..128{if style&!STYLE_MASK==0{let k=SlotCache::key(char::MAX,style).unwrap();assert_ne!(k,EMPTY);c.insert(char::MAX,style,513);assert_eq!(c.get(char::MAX,style),Some(513));}else{c.insert('a',style,9);assert_eq!(c.get('a',style),None);}}assert_eq!(SlotCache::payload_bytes(),4096);assert_eq!(SlotCache::key('a',256),None);}
    #[test]fn collisions_replace_and_reset_never_replay_old_slot(){let mut c=SlotCache::new();let first='A';let bucket=SlotCache::index(SlotCache::key(first,0).unwrap());let other=(1..=0x10ffff).filter_map(char::from_u32).find(|ch|*ch!=first&&SlotCache::index(SlotCache::key(*ch,0).unwrap())==bucket).unwrap();c.insert(first,0,300);assert_eq!(c.get(first,0),Some(300));c.insert(other,0,511);assert_eq!(c.get(first,0),None);assert_eq!(c.get(other,0),Some(511));c.clear();assert_eq!(c.get(other,0),None);c.insert(first,0,1);assert_eq!(c.get(first,0),Some(1));assert_eq!(c.get(other,0),None);}
}
