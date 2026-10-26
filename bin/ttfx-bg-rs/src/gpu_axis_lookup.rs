//! Exact pixel-owner maps, rebuilt per changed axis; normalized color sampling untouched.
const MAX_AXIS_PIXELS:i32=65536;
pub fn eligible(size:i32,count:i32,max_texture:i32)->bool{size>0&&count>0&&size<=max_texture&&size<=MAX_AXIS_PIXELS}
pub fn map_axis(size:i32,count:i32)->Vec<u32>{
    assert!(size>0&&count>0&&size<=MAX_AXIS_PIXELS);
    let(size,n)=(size as u64,count as u64);let mut out=Vec::with_capacity(size as usize*2);
    for p in 0..size{let cell=((p+1)*n-1)/size;let origin=cell*size/n;out.extend_from_slice(&[cell as u32,(p-origin)as u32]);}
    out
}
pub fn fragment(reference:&str)->String{
    let start=reference.find("    vec2 px = floor(p);").unwrap();let end=reference.find("    vec4 fg = texelFetch").unwrap();
    let fallback=reference[start..end].replace("ivec2 local =","local =").replace("ivec2 ic =","ic =");
    let mapping=format!("    ivec2 local; ivec2 ic;\n    if (u_axis_enabled != 0) {{\n        uvec2 x=texelFetch(u_axis_x,ivec2(int(p.x),0),0).rg;\n        uvec2 y=texelFetch(u_axis_y,ivec2(int(p.y),0),0).rg;\n        ic=ivec2(x.r,y.r);local=ivec2(x.g,y.g);\n    }} else {{\n{fallback}    }}\n");
    let mut source=reference.to_owned();source.replace_range(start..end,&mapping);
    source.replacen("out vec4 frag;","uniform highp usampler2D u_axis_x;\nuniform highp usampler2D u_axis_y;\nuniform int u_axis_enabled;\nout vec4 frag;",1)
}
pub struct AxisLookup {textures:[u32;2],keys:[Option<(i32,i32)>;2],max:i32,enabled:i32,pub active:bool,pub upload_bytes:usize,pub rebuilds:u32,pub resize_cpu_ns:Option<u64>,pub resize_wall_ns:Option<u64>,measure:bool,failed_geometry:Option<[(i32,i32);2]>,fallback:Option<&'static str>,#[cfg(test)]fail_upload:bool}
impl AxisLookup{
    pub unsafe fn new(program:u32)->Result<Self,String>{
        use std::ffi::CString;
        let location=|n:&str|gl::GetUniformLocation(program,CString::new(n).unwrap().as_ptr());
        let mut max=0;gl::GetIntegerv(gl::MAX_TEXTURE_SIZE,&mut max);
        let mut units=0;gl::GetIntegerv(gl::MAX_TEXTURE_IMAGE_UNITS,&mut units);
        if max<1 || units<5 { return Err("axis lookup texture capabilities unavailable".into()); }
        gl::Uniform1i(location("u_axis_x"),3);gl::Uniform1i(location("u_axis_y"),4);
        let mut textures=[0;2];gl::GenTextures(2,textures.as_mut_ptr());
        for tex in textures{gl::BindTexture(gl::TEXTURE_2D,tex);gl::TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MIN_FILTER,gl::NEAREST as i32);gl::TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MAG_FILTER,gl::NEAREST as i32);gl::TexImage2D(gl::TEXTURE_2D,0,gl::RG32UI as i32,1,1,0,gl::RG_INTEGER,gl::UNSIGNED_INT,[0u32;2].as_ptr().cast());}
        let error=gl::GetError();
        if error!=gl::NO_ERROR || textures.contains(&0) {gl::DeleteTextures(2,textures.as_ptr());return Err(format!("axis initialization GL error {error:#x}"));}
        Ok(Self{textures,keys:[None,None],max,enabled:location("u_axis_enabled"),active:false,upload_bytes:0,rebuilds:0,resize_cpu_ns:None,resize_wall_ns:None,measure:std::env::var_os("TTFX_BG_PRESENTATION_LOG").is_some(),failed_geometry:None,fallback:None,#[cfg(test)]fail_upload:false})
    }
    pub unsafe fn ensure(&mut self,width:i32,height:i32,cols:i32,rows:i32){
        self.upload_bytes=0;self.rebuilds=0;self.resize_cpu_ns=None;self.resize_wall_ns=None;
        self.active=eligible(width,cols,self.max)&&eligible(height,rows,self.max);
        self.fallback=(!self.active).then_some("axis_limits");
        if !self.active{return}
        let pairs=[(width,cols),(height,rows)];
        if self.failed_geometry==Some(pairs) {self.active=false;self.fallback=Some("axis_upload_gl_error");return;}
        self.failed_geometry=None;
        for(i,(size,n))in pairs.into_iter().enumerate(){
            if self.keys[i]==Some((size,n)){continue}
            let begin=self.measure.then(||(crate::producer_probe::thread_cpu_ns(),std::time::Instant::now()));
            let data=map_axis(size,n);self.upload_bytes+=data.len()*std::mem::size_of::<u32>();self.rebuilds+=1;
            gl::BindTexture(gl::TEXTURE_2D,self.textures[i]);
            gl::TexImage2D(gl::TEXTURE_2D,0,gl::RG32UI as i32,size,1,0,gl::RG_INTEGER,gl::UNSIGNED_INT,data.as_ptr().cast());
            let error=gl::GetError();
            #[cfg(test)] let error=if self.fail_upload {gl::OUT_OF_MEMORY}else{error};
            if error!=gl::NO_ERROR { self.active=false;self.fallback=Some("axis_upload_gl_error");self.keys[i]=None;self.failed_geometry=Some(pairs);return; }
            self.keys[i]=Some((size,n));
            if let Some((cpu,wall))=begin{*self.resize_cpu_ns.get_or_insert(0)+=crate::producer_probe::thread_cpu_ns()-cpu;*self.resize_wall_ns.get_or_insert(0)+=wall.elapsed().as_nanos()as u64;}
        }
    }
    pub unsafe fn bind(&self){gl::Uniform1i(self.enabled,self.active as i32);for(i,tex)in self.textures.into_iter().enumerate(){gl::ActiveTexture(gl::TEXTURE3+i as u32);gl::BindTexture(gl::TEXTURE_2D,tex);}}
    // Caller guarantees the owning pipeline's context is current.
    pub unsafe fn release_current(&mut self){gl::DeleteTextures(2,self.textures.as_ptr());self.textures=[0;2];}
    pub fn fallback_reason(&self)->Option<&'static str>{self.fallback}
    #[cfg(test)]pub fn textures_for_test(&self)->[u32;2]{self.textures}
    #[cfg(test)]pub fn fail_upload_for_test(&mut self,fail:bool){self.fail_upload=fail;}
    #[cfg(test)]pub fn max_for_test(&self)->i32{self.max}
    #[cfg(test)]pub fn set_max_for_test(&mut self,max:i32){self.max=max;}
    pub fn payload_bytes(&self)->usize{self.keys.iter().map(|k|k.map_or(8,|(size,_)|size as usize*8)).sum()}
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn maps_exact_integer_partitions_and_degenerate_grids(){for(size,n)in[(2560,237),(1600,61),(321,20),(1,100000),(17,80),(16384,5461),(65536,i32::MAX),(97,1)]{let values=map_axis(size,n);for p in 0..size{let c=values[p as usize*2]as u64;let local=values[p as usize*2+1]as u64;let lo=c*size as u64/n as u64;let hi=(c+1)*size as u64/n as u64;assert!(lo<=p as u64&&(p as u64)<hi);assert_eq!(local,p as u64-lo);}}}
    #[test]fn invalid_or_excessive_maps_fall_back_before_allocation(){for(s,n,max)in[(0,10,4096),(-1,10,4096),(10,0,4096),(4097,10,4096),(65537,1,100000),(1,1,0)]{assert!(!eligible(s,n,max));}assert!(eligible(2560,237,4096));}
}
