//! Opt-in asynchronous GPU timestamps. No GL call is made by the disabled path.
//! https://registry.khronos.org/OpenGL/extensions/EXT/EXT_disjoint_timer_query.txt
use std::{collections::VecDeque,ffi::{c_void,CStr,CString}};
use serde_json::{json,Value};
const TIMESTAMP:u32=0x8e28;const BITS:u32=0x8864;const RESULT:u32=0x8866;
const AVAILABLE:u32=0x8867;const DISJOINT:u32=0x8fbb;const CAPACITY:usize=64;
type Gen=unsafe extern "system" fn(i32,*mut u32);
type Delete=unsafe extern "system" fn(i32,*const u32);
type Counter=unsafe extern "system" fn(u32,u32);
type GetQuery=unsafe extern "system" fn(u32,u32,*mut i32);
type GetObject=unsafe extern "system" fn(u32,u32,*mut u32);
type Get64=unsafe extern "system" fn(u32,u32,*mut u64);
struct Api {delete:Delete,counter:Counter,get_object:GetObject,get64:Get64}
#[derive(Clone,Copy)]struct Batch {ids:[u32;3],frame:u64}
trait Readback {fn available(&mut self,id:u32)->bool;fn value(&mut self,id:u32)->u64;fn disjoint(&mut self)->bool;}
struct Reader<'a>(&'a Api);
impl Readback for Reader<'_>{
    fn available(&mut self,id:u32)->bool{let mut v=0;unsafe{(self.0.get_object)(id,AVAILABLE,&mut v)};v!=0}
    fn value(&mut self,id:u32)->u64{let mut v=0;unsafe{(self.0.get64)(id,RESULT,&mut v)};v}
    fn disjoint(&mut self)->bool{let mut v=0;unsafe{gl::GetIntegerv(DISJOINT,&mut v)};v!=0}
}
#[derive(Default)]struct Queue {free:Vec<[u32;3]>,pending:VecDeque<Batch>}
impl Queue {
    fn invalidate(&mut self)->usize{let n=self.pending.len();while let Some(b)=self.pending.pop_front(){self.free.push(b.ids)}n}
    fn collect(&mut self,r:&mut impl Readback)->(Vec<(u64,[u64;3])>,usize){
        if r.disjoint(){let n=self.invalidate();return(vec![],n)}
        let mut ready=vec![];
        // Bounded scan, no retry loop: only results whose LAST timestamp is
        // available. Same-type earlier timestamp queries are then available too.
        while let Some(batch)=self.pending.front(){
            if !r.available(batch.ids[2]){break}
            let batch=self.pending.pop_front().unwrap();
            let values=batch.ids.map(|id|r.value(id));self.free.push(batch.ids);
            ready.push((batch.frame,values));
        }
        // Do not publish any candidate value before the closing disjoint check.
        if r.disjoint(){let n=ready.len()+self.invalidate();(vec![],n)}else{(ready,0)}
    }
}
pub struct Timer {api:Api,all:Vec<u32>,queue:Queue,current:Option<Batch>,sequence:u64,events:Vec<Value>,disabled:bool}
unsafe fn symbol<T:Copy>(name:&str)->Result<T,String>{
    let c=CString::new(name).unwrap();let p=super::gpu_grid::egl_timer_proc(c.as_ptr());
    if p.is_null(){return Err(format!("missing {name}"))}
    Ok(std::mem::transmute_copy::<*const c_void,T>(&p))
}
unsafe fn gl_string(name:u32)->String{let p=gl::GetString(name);if p.is_null(){"unavailable".into()}else{CStr::from_ptr(p.cast()).to_string_lossy().into_owned()}}
impl Timer {
    /// Caller must hold the current context. Explicitly called ONLY by a logged
    /// diagnostic path (or an explicit EGL test), never by normal construction.
    pub unsafe fn new()->Result<Self,String>{
        let mut count=0;gl::GetIntegerv(gl::NUM_EXTENSIONS,&mut count);
        let supported=(0..count.max(0)as u32).any(|i|{let p=gl::GetStringi(gl::EXTENSIONS,i);!p.is_null()&&CStr::from_ptr(p.cast()).to_bytes()==b"GL_EXT_disjoint_timer_query"});
        if !supported{return Err("GL_EXT_disjoint_timer_query unavailable in this current context".into())}
        let gen:Gen=symbol("glGenQueriesEXT")?;let get:GetQuery=symbol("glGetQueryivEXT")?;
        let api=Api{delete:symbol("glDeleteQueriesEXT")?,counter:symbol("glQueryCounterEXT")?,get_object:symbol("glGetQueryObjectuivEXT")?,get64:symbol("glGetQueryObjectui64vEXT")?};
        let mut bits=0;get(TIMESTAMP,BITS,&mut bits);
        if bits<=0{return Err("timestamp counter has zero useful bits".into())}
        let error=gl::GetError();if error!=gl::NO_ERROR{return Err(format!("capability GL error {error:#x}"))}
        let mut all=vec![0;CAPACITY*3];gen(all.len()as i32,all.as_mut_ptr());
        let mut initial_disjoint=0;gl::GetIntegerv(DISJOINT,&mut initial_disjoint);
        let error=gl::GetError();if error!=0||all.contains(&0){(api.delete)(all.len()as i32,all.as_ptr());return Err(format!("query allocation GL error {error:#x}"))}
        let queue=Queue{free:all.chunks_exact(3).map(|s|[s[0],s[1],s[2]]).collect(),pending:VecDeque::new()};
        let capability=json!({"event":"gpu_timer_capability","available":true,"renderer":gl_string(gl::RENDERER),"version":gl_string(gl::VERSION),"timestamp_bits":bits,"initial_disjoint":initial_disjoint,"capacity_frames":CAPACITY,"method":"EXT timestamp differences; asynchronous; excludes EGL/compositor"});
        Ok(Self{api,all,queue,current:None,sequence:0,events:vec![capability],disabled:false})
    }
    pub unsafe fn begin(&mut self){
        if self.disabled{return}
        let(ready,invalid)=self.queue.collect(&mut Reader(&self.api));
        if invalid>0{self.events.push(json!({"event":"gpu_timer_disjoint","invalidated_frames":invalid}));}
        for(frame,v)in ready{
            if v[0]<=v[1]&&v[1]<=v[2]{self.events.push(json!({"event":"gpu_elapsed","gpu_timing_frame":frame,"gpu_upload_us":(v[1]-v[0])as f64/1000.,"gpu_draw_us":(v[2]-v[1])as f64/1000.,"gpu_total_us":(v[2]-v[0])as f64/1000.}));}
            else{self.events.push(json!({"event":"gpu_timer_invalid","gpu_timing_frame":frame,"reason":"timestamp_order_or_wrap"}));}
        }
        let error=gl::GetError();if error!=0{self.disabled=true;self.events.clear();self.events.push(json!({"event":"gpu_timer_unavailable","reason":"readback_gl_error","gl_error":error}));return}
        self.sequence+=1;
        self.current=self.queue.free.pop().map(|ids|Batch{ids,frame:self.sequence});
        if let Some(b)=self.current{(self.api.counter)(b.ids[0],TIMESTAMP)}
        else{self.events.push(json!({"event":"gpu_timer_saturated","gpu_timing_frame":self.sequence,"pending_frames":self.queue.pending.len()}));}
    }
    pub unsafe fn after_upload(&mut self){if let Some(b)=self.current{(self.api.counter)(b.ids[1],TIMESTAMP)}}
    pub unsafe fn end(&mut self){if let Some(b)=self.current.take(){(self.api.counter)(b.ids[2],TIMESTAMP);self.queue.pending.push_back(b);}}
    pub fn frame(&self)->Option<u64>{(!self.disabled).then_some(self.sequence)}
    pub fn take_events(&mut self)->Vec<Value>{std::mem::take(&mut self.events)}
    /// No final wait/readback: pending timings are explicitly reported missing.
    pub unsafe fn finish(mut self)->Vec<Value>{self.events.push(json!({"event":"gpu_timer_end","pending_unread_frames":self.queue.pending.len()}));(self.api.delete)(self.all.len()as i32,self.all.as_ptr());self.events}
}
#[cfg(test)]mod tests{
    use super::*;
    struct Fake {available:bool,disjoint_at:usize,checks:usize,reads:usize}
    impl Readback for Fake{
        fn available(&mut self,_:u32)->bool{self.available}
        fn value(&mut self,id:u32)->u64{assert!(self.available,"must never read unavailable result");self.reads+=1;id as u64*100}
        fn disjoint(&mut self)->bool{self.checks+=1;self.checks==self.disjoint_at}
    }
    fn queue()->Queue{Queue{free:vec![],pending:(0..CAPACITY).map(|i|Batch{ids:[i as u32*3+1,i as u32*3+2,i as u32*3+3],frame:i as u64}).collect()}}
    #[test]fn unavailable_is_never_read_and_pending_storage_is_bounded(){let mut q=queue();let mut f=Fake{available:false,disjoint_at:0,checks:0,reads:0};let(r,n)=q.collect(&mut f);assert!(r.is_empty());assert_eq!((n,f.reads,q.pending.len(),q.free.len()),(0,0,CAPACITY,0));}
    #[test]fn available_results_recycle_only_after_readback(){let mut q=queue();let mut f=Fake{available:true,disjoint_at:0,checks:0,reads:0};let(r,n)=q.collect(&mut f);assert_eq!((r.len(),n,f.reads,q.free.len()),(CAPACITY,0,CAPACITY*3,CAPACITY));assert!(q.pending.is_empty());assert_eq!(r[0],(0,[100,200,300]));}
    #[test]fn disjoint_before_or_after_reads_discards_all_candidate_values(){for disjoint_at in[1,2]{let mut q=queue();let mut f=Fake{available:true,disjoint_at,checks:0,reads:0};let(r,n)=q.collect(&mut f);assert!(r.is_empty());assert_eq!(n,CAPACITY);assert_eq!(q.free.len(),CAPACITY);assert!(q.pending.is_empty());}}
}
