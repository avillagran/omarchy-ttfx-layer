//! Reflejo exacto de los bytes enviados a cada textura, no del último fill CPU.
#[derive(Default)]
pub struct UploadCache {size:(i32,i32),valid:bool,fg:Vec<u8>,bg:Vec<u8>}
impl UploadCache {
    pub fn plan(&self,size:(i32,i32),fg:&[u8],bg:&[u8],force:bool)->[bool;2] {
        if force || !self.valid || self.size!=size {[true,true]}
        else {[self.fg!=fg,self.bg!=bg]}
    }
    pub fn commit(&mut self,size:(i32,i32),fg:&[u8],bg:&[u8],sent:[bool;2]) {
        if sent[0] {self.fg.clear();self.fg.extend_from_slice(fg);}
        if sent[1] {self.bg.clear();self.bg.extend_from_slice(bg);}
        self.size=size;self.valid=true;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_policy_keeps_gpu_exact_across_skips_flags_and_reinitialization() {
        let mut cache=UploadCache::default();let mut gpu=[vec![],vec![]];
        let cases=[((2,1),vec![1,2,3,1,4,5,6,2],vec![0;8],false),
          ((2,1),vec![9,2,3,1,4,5,6,2],vec![0;8],false),
          ((2,1),vec![9,2,3,1,4,5,6,2],vec![0,0,0,2,1,2,3,1],false),
          ((2,1),vec![9,2,3,1,4,5,6,2],vec![0,0,0,2,1,2,3,1],false),
          ((2,1),vec![9,2,3,1,4,5,6,2],vec![0,0,0,2,1,2,3,1],true),
          ((1,2),vec![0;8],vec![0;8],false)];
        let expected=[[true,true],[true,false],[false,true],[false,false],[true,true],[true,true]];
        for ((size,fg,bg,force),want) in cases.into_iter().zip(expected) {
            let sent=cache.plan(size,&fg,&bg,force);assert_eq!(sent,want);
            if sent[0]{gpu[0]=fg.clone();}if sent[1]{gpu[1]=bg.clone();}
            cache.commit(size,&fg,&bg,sent);assert_eq!(gpu,[fg,bg]);
        }
    }
    #[test]
    fn unpublished_cpu_changes_never_replace_uploaded_reference() {
        let mut cache=UploadCache::default();
        let size=(1,1);let a=[1,2,3,0];let b=[9,2,3,0];let bg=[0;4];
        let sent=cache.plan(size,&a,&bg,false);cache.commit(size,&a,&bg,sent);
        assert_eq!(cache.plan(size,&b,&bg,false),[true,false]); // No draw/commit de B.
        assert_eq!(cache.plan(size,&a,&bg,false),[false,false]);
    }
}
