//! Gate GPU explícito, sin Wayland/desktop. No se ejecuta en suite normal.
use super::*;
use std::ffi::CStr;
type Handle=*mut c_void;
#[link(name="EGL")]
extern "C" {
    fn eglGetPlatformDisplay(platform:u32,native:Handle,attrs:*const isize)->Handle;
    fn eglInitialize(display:Handle,major:*mut i32,minor:*mut i32)->u32;
    fn eglBindAPI(api:u32)->u32;
    fn eglChooseConfig(display:Handle,attrs:*const i32,config:*mut Handle,count:i32,n:*mut i32)->u32;
    fn eglCreatePbufferSurface(display:Handle,config:Handle,attrs:*const i32)->Handle;
    fn eglCreateContext(display:Handle,config:Handle,share:Handle,attrs:*const i32)->Handle;
    fn eglMakeCurrent(display:Handle,draw:Handle,read:Handle,ctx:Handle)->u32;
    fn eglDestroyContext(display:Handle,ctx:Handle)->u32;
    fn eglDestroySurface(display:Handle,surface:Handle)->u32;
    fn eglTerminate(display:Handle)->u32;
}
struct Sandbox {display:Handle,surface:Handle,context:Handle,config:Handle}
impl Sandbox {
    unsafe fn new()->Self {
        let display=eglGetPlatformDisplay(0x31DD,std::ptr::null_mut(),std::ptr::null());
        assert!(!display.is_null(),"EGL surfaceless no disponible");
        let(mut major,mut minor)=(0,0);
        assert_ne!(eglInitialize(display,&mut major,&mut minor),0,"eglInitialize falló");
        assert_ne!(eglBindAPI(0x30A0),0);
        let attrs=[0x3033,1,0x3040,0x40,0x3024,8,0x3023,8,0x3022,8,0x3021,8,0x3038];
        let mut config=std::ptr::null_mut();let mut count=0;
        assert_ne!(eglChooseConfig(display,attrs.as_ptr(),&mut config,1,&mut count),0);
        assert!(count>0,"Sin config GLES3/pbufferRGBA8");
        let surface=eglCreatePbufferSurface(display,config,[0x3057,2560,0x3056,1600,0x3038].as_ptr());
        assert!(!surface.is_null());
        let context=eglCreateContext(display,config,std::ptr::null_mut(),[0x3098,3,0x3038].as_ptr());
        assert!(!context.is_null());
        assert_ne!(eglMakeCurrent(display,surface,surface,context),0);
        load_gl();
        let renderer=gl::GetString(gl::RENDERER);
        assert!(!renderer.is_null());
        eprintln!("GPU equivalence renderer: {}",CStr::from_ptr(renderer.cast()).to_string_lossy());
        Self{display,surface,context,config}
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {unsafe {
        eglMakeCurrent(self.display,std::ptr::null_mut(),std::ptr::null_mut(),std::ptr::null_mut());
        eglDestroyContext(self.display,self.context);eglDestroySurface(self.display,self.surface);eglTerminate(self.display);
    }}
}
fn assert_pixels_equal(a:&[Vec<u8>],b:&[Vec<u8>],label:&str) {
    assert_eq!(a.len(),b.len(),"cantidad de frames {label}");
    for (frame,(a,b)) in a.iter().zip(b).enumerate() {
        assert_eq!(a.len(),b.len(),"tamaño RGBA frame{frame} {label}");
        if let Some(byte)=a.iter().zip(b).position(|(a,b)|a!=b) {
            let first=byte/4*4;let end=(first+4).min(a.len());
            panic!("{label}: frame{frame} byte{byte}, RGBA ref={:?} candidato={:?}",&a[first..end],&b[first..end]);
        }
    }
}
unsafe fn frames(profile:RenderProfile)->(Vec<Vec<u8>>,Vec<[usize;2]>) {let(a,b,_)=frames_timing(profile,false);(a,b)}
unsafe fn frames_timing(profile:RenderProfile,timing:bool)->(Vec<Vec<u8>>,Vec<[usize;2]>,Vec<serde_json::Value>) {
    let mut pipeline=GpuPipeline::with_profile(true,profile).unwrap();
    if timing{pipeline.enable_timing();}
    let mut timer_events=vec![];
    let mut grid=GpuGrid::with_profile(20,20,8.,profile);
    let cells:Vec<DirectCell>=(0..400).map(|i|DirectCell{
        x:i%20,y:i/20,ch:char::from_u32(0x100+i as u32).unwrap(),
        color:((i%251)as u8,77,180),background:if i%3==0{Some((19,38,57))}else{None},
        attrs:[0,1,2,3,4,64,67,71,32,128][i%10],
    }).collect();
    let mut snapshots=vec![];let mut uploads=vec![];
    gl::ClearColor(0.,0.,0.,0.);gl::Clear(gl::COLOR_BUFFER_BIT);
    for step in 0..9 {
        let(w,h)=if step<7{(320,240)}else{(321,241)};
        gl::Viewport(0,0,w,h);grid.set_size(w,h,1.);
        let mut frame=cells.clone();
        if step>=1 {frame[0].color=(250,2,3);}
        if step>=2 {frame[399].ch=frame[0].ch;frame[399].attrs=frame[0].attrs;}
        if step>=3 {frame[0].background=Some((9,8,7));}
        if step==5 {frame.clear();}
        if step==6 {grid.reset_atlas();}
        grid.fill(&frame).unwrap();
        // Mantener la ruta damage aunque no se optimiza el repintado en este gate.
        let _=grid.damage_tiles();
        pipeline.draw(&mut grid,true,step%2==0,None);
        uploads.push(pipeline.last_cell_upload_bytes());
        timer_events.extend(pipeline.take_timing_events());
        let mut pixels=vec![0;w as usize*h as usize*4];
        gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,pixels.as_mut_ptr().cast());
        assert_eq!(gl::GetError(),gl::NO_ERROR,"GL error step {step}");
        snapshots.push(pixels);
    }
    timer_events.extend(pipeline.finish_timing());
    (snapshots,uploads,timer_events)
}
#[test]
#[ignore="combined profiles: exact normalized RGBA, Unicode/styles/high slots, erase/reset/resize"]
fn optimized_profiles_match_reference_pixels() {unsafe {
    let _context=Sandbox::new();
    let(a,uploads)=frames(RenderProfile::Reference);
    for profile in [RenderProfile::Clipped,RenderProfile::Axis] {
        let(b,sent)=frames(profile);
        assert_pixels_equal(&a,&b,profile.name());assert_eq!(uploads,sent);
    }
}}

unsafe fn seam_frames(profile:RenderProfile,force_fallback:bool)->Vec<Vec<u8>> {
    let mut snapshots=vec![];
    for (cols,rows,w,h) in [(237,61,2560,1600),(237,61,509,337),(80,24,17,13),(1,1,97,59),(80,24,511,383)] {
        let mut pipeline=GpuPipeline::with_profile(true,profile).unwrap();
        let mut grid=GpuGrid::with_profile(cols,rows,8.,profile);grid.set_size(w,h,1.);
        let glyphs=['A','█','あ',' ','_','界','M'];
        let mut cells:Vec<DirectCell>=(0..cols*rows).map(|i|DirectCell {
            x:i%cols,y:i/cols,ch:glyphs[i%glyphs.len()],color:((i%255)as u8,120,220),
            background:if i%3==0{Some((30,60,90))}else{None},attrs:if i%5==0{4}else{0},
        }).collect();
        gl::Viewport(0,0,w,h);gl::ClearColor(0.,0.,0.,0.);gl::Clear(gl::COLOR_BUFFER_BIT);
        for step in 0..3 {
            if step==1 {cells[0].color=(250,0,1);}
            if step==2 {let i=cells.len()/2;cells[i].ch='█';cells[i].background=None;}
            grid.fill(&cells).unwrap();
            let damage=grid.damage();
            let opaque=step!=2;
            pipeline.draw(&mut grid,true,opaque,damage.as_deref());
            if force_fallback {
                // Ejercitar el programa B con fallback fullscreen explícito.
                gl::UseProgram(pipeline.program);gl::Uniform1i(pipeline.u_cells_mode,0);
                gl::BindVertexArray(pipeline.vao);gl::DrawArrays(gl::TRIANGLES,0,3);gl::BindVertexArray(0);
            }
            assert_eq!(gl::GetError(),gl::NO_ERROR);
            let mut pixels=vec![0;w as usize*h as usize*4];
            gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,pixels.as_mut_ptr().cast());
            assert_eq!(gl::GetError(),gl::NO_ERROR);snapshots.push(pixels);
        }
    }
    snapshots
}
#[test]
#[ignore="combined profiles: exact cell boundaries, subpixel grids, alpha and shader fallback"]
fn optimized_profiles_preserve_seams_and_clipped_fallback() {unsafe {
    let _context=Sandbox::new();
    let baseline=seam_frames(RenderProfile::Reference,false);
    for profile in [RenderProfile::Clipped,RenderProfile::Axis] {
        assert_pixels_equal(&baseline,&seam_frames(profile,false),profile.name());
    }
    let fallback=seam_frames(RenderProfile::Clipped,true);
    for i in (0..baseline.len()).step_by(3) {assert_pixels_equal(std::slice::from_ref(&baseline[i]),std::slice::from_ref(&fallback[i]),"clipped shader reference fallback");}
}}

/// Buffers previos coherentes: sólo se altera contenido cuya celda entera está
/// incluida en el clip. El full redraw sirve de oráculo también fuera del daño.
unsafe fn coherent_clips(profile:RenderProfile,full_redraw:bool)->(Vec<Vec<u8>>,u64) {
    let mut snapshots=vec![];let mut vertices=0;
    for (cols,rows,w,h) in [(237,61,2560,1600),(237,61,509,337),(80,24,17,13),(1,1,97,59),(80,24,511,383)] {
        for opaque in [false,true] {
            let mut pipeline=GpuPipeline::with_profile(true,profile).unwrap();
            let mut grid=GpuGrid::with_profile(cols,rows,8.,profile);grid.set_size(w,h,1.);
            let mut cells:Vec<DirectCell>=(0..cols*rows).map(|i|DirectCell {
                x:i%cols,y:i/cols,ch:char::from_u32(0x100+(i%400)as u32).unwrap(),
                color:((i%255)as u8,120,220),background:if i%3==0{Some((30,60,90))}else{None},attrs:if i%5==0{4}else{0},
            }).collect();
            gl::Viewport(0,0,w,h);gl::ClearColor(0.13,0.21,0.34,0.55);gl::Clear(gl::COLOR_BUFFER_BIT);
            for step in 0..5 {
                let cell=match step {1=>0,2=>cells.len()/2,_=>cells.len()-1};
                if step>0 {cells[cell].color=(step*43,2,255-step*17);cells[cell].background=Some((step,55,100));}
                let x=cells[cell].x as i32;let y=cells[cell].y as i32;
                let x0=x*w/cols as i32;let x1=(x+1)*w/cols as i32;
                let y0=y*h/rows as i32;let y1=(y+1)*h/rows as i32;
                let clips=[[x0,y0,x1-x0,y1-y0],[w/3,0,1,h],[0,h/3,w,1],[0,0,1,1],[w-1,h-1,1,1],[w,0,0,h]];
                grid.fill(&cells).unwrap();let _=grid.damage_tiles();
                pipeline.draw(&mut grid,true,opaque,if step==0||full_redraw{None}else{Some(&clips)});
                vertices+=pipeline.last_draw_totals()[1];
                assert_eq!(gl::GetError(),gl::NO_ERROR,"clip profile{profile:?} step{step}");
                let mut pixels=vec![0;w as usize*h as usize*4];
                gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,pixels.as_mut_ptr().cast());
                assert_eq!(gl::GetError(),gl::NO_ERROR);snapshots.push(pixels);
            }
        }
    }
    (snapshots,vertices)
}
#[test]
#[ignore="combined R26+backend exact full redraw oracle, overlaps and pixels outside damage"]
fn optimized_profiles_preserve_coherent_clips() {unsafe {
    let _context=Sandbox::new();
    let(full,_)=coherent_clips(RenderProfile::Reference,true);
    let(reference,_)=coherent_clips(RenderProfile::Reference,false);
    assert_pixels_equal(&full,&reference,"coherent original oracle");
    for profile in [RenderProfile::Clipped,RenderProfile::Axis] {
        let(candidate,_)=coherent_clips(profile,false);
        assert_pixels_equal(&reference,&candidate,profile.name());
    }
}}

#[test]
#[ignore="explicit GPU timer capability/equivalence; not a resource benchmark"]
fn gpu_timer_is_async_and_preserves_reference_pixels(){unsafe{
    let _context=Sandbox::new();
    let(a,_,off)=frames_timing(RenderProfile::Reference,false);assert!(off.is_empty(),"OFF must not initialize/query timer");
    let(b,_,events)=frames_timing(RenderProfile::Reference,true);
    assert_pixels_equal(&a,&b,"GPU timestamp instrumentation must preserve exact images");
    for e in &events{eprintln!("GPU_TIMER_GATE {e}");}
    assert!(events.iter().any(|e|e["event"]=="gpu_timer_capability"&&e["available"]==true),"timer unavailable in this context; do not claim timing");
    assert!(events.iter().any(|e|e["event"]=="gpu_elapsed"),"no asynchronously completed valid samples");
    assert!(!events.iter().any(|e|e["event"]=="gpu_timer_unavailable"||e["event"]=="gpu_timer_invalid"));
}}

#[test]
#[ignore="exact axis lookup on three coherent previous buffers, age2/3, erase and max-texture fallback"]
fn optimized_profiles_preserve_buffer_age_and_fallback(){unsafe{
    let _context=Sandbox::new();
    for profile in [RenderProfile::Clipped,RenderProfile::Axis] {
    let mut reference=GpuPipeline::with_profile(true,RenderProfile::Reference).unwrap();
    let mut candidate=GpuPipeline::with_profile(true,profile).unwrap();
    let(mut a,mut b)=(GpuGrid::with_profile(80,24,8.,RenderProfile::Reference),GpuGrid::with_profile(80,24,8.,profile));
    let mut cells:Vec<DirectCell>=(0..80*24).map(|i|DirectCell{x:i%80,y:i/80,ch:char::from_u32(0x100+(i%400)as u32).unwrap(),color:((i%251)as u8,77,180),background:if i%3==0{Some((19,38,57))}else{None},attrs:if i%7==0{4}else{0}}).collect();
    let(mut textures,mut fbos)=([0;4],[0;4]);gl::GenTextures(4,textures.as_mut_ptr());gl::GenFramebuffers(4,fbos.as_mut_ptr());
    let mut ages=vec![];
    let max=candidate.axis.as_ref().map(|axis|axis.max_for_test());
    for opaque in [true,false]{
    let mut previous=[None;3];let mut history:Vec<[i32;4]>=vec![];
    for step in 0..12usize{
        let(w,h)=if step<6{(511,383)}else if step<9{(509,337)}else{(509,339)};
        if [0,6,9].contains(&step){
            previous=[None;3];
            for i in 0..4{gl::ActiveTexture(gl::TEXTURE0);gl::BindTexture(gl::TEXTURE_2D,textures[i]);gl::TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MIN_FILTER,gl::NEAREST as i32);gl::TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MAG_FILTER,gl::NEAREST as i32);gl::TexImage2D(gl::TEXTURE_2D,0,gl::RGBA8 as i32,w,h,0,gl::RGBA,gl::UNSIGNED_BYTE,std::ptr::null());gl::BindFramebuffer(gl::FRAMEBUFFER,fbos[i]);gl::FramebufferTexture2D(gl::FRAMEBUFFER,gl::COLOR_ATTACHMENT0,gl::TEXTURE_2D,textures[i],0);assert_eq!(gl::CheckFramebufferStatus(gl::FRAMEBUFFER),gl::FRAMEBUFFER_COMPLETE);}
        }
        let index=if step%2==0{0}else{cells.len()-1};
        cells[index].ch=if step%3==0{' '}else{'界'};cells[index].color=(step as u8*19,120,88);cells[index].background=if step%2==0{None}else{Some((2,3,4))};
        let(x,y)=(cells[index].x as i32,cells[index].y as i32);let(x0,x1,y0,y1)=(x*w/80,(x+1)*w/80,y*h/24,(y+1)*h/24);
        history.push([x0,y0,x1-x0,y1-y0]);
        a.set_size(w,h,1.);b.set_size(w,h,1.);a.fill(&cells).unwrap();b.fill(&cells).unwrap();gl::Viewport(0,0,w,h);
        gl::BindFramebuffer(gl::FRAMEBUFFER,fbos[0]);reference.draw(&mut a,true,opaque,None);
        let mut expected=vec![0;w as usize*h as usize*4];gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,expected.as_mut_ptr().cast());
        let slot=[0,1,2,0,2,1,0,1,2,0,2,1][step];let clips=previous[slot].map(|last:usize|{ages.push(step-last);history[last+1..=step].to_vec()});
        // Force reference fallback without exceeding a real context allocation.
        if let Some(axis)=candidate.axis.as_mut(){axis.set_max_for_test(if step==4{0}else{max.unwrap()});}
        gl::BindFramebuffer(gl::FRAMEBUFFER,fbos[slot+1]);candidate.draw(&mut b,true,opaque,clips.as_deref());previous[slot]=Some(step);
        if let Some(axis)=candidate.axis.as_ref(){
            assert_eq!(axis.active,step!=4);
            if ![0,6,9].contains(&step){assert_eq!(axis.upload_bytes,0,"stable or fallback must not rebuild");}
            else{assert_eq!(axis.rebuilds,if step==9{1}else{2});}
            assert_eq!(candidate.profile_status(&b)["fallback"].is_null(),step!=4);
        }
        assert_eq!(a.fg,b.fg);assert_eq!(a.bg,b.bg);assert_eq!(a.slots,b.slots);
        let mut got=vec![0;expected.len()];gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,got.as_mut_ptr().cast());assert_eq!(gl::GetError(),gl::NO_ERROR);
        assert_pixels_equal(&[expected],&[got],"axis age3/coherent outside union/erase/resize/fallback");
    }
    }
    assert!(ages.contains(&2)&&ages.contains(&3));
    gl::BindFramebuffer(gl::FRAMEBUFFER,0);gl::DeleteFramebuffers(4,fbos.as_ptr());gl::DeleteTextures(4,textures.as_ptr());
    }
}}

#[test]
#[ignore="product profiles ignore inherited trial switches; constructor route and effective metadata"]
fn optimized_profiles_ignore_trials_and_report_effective_route(){unsafe{
    let _context=Sandbox::new();
    let old=crate::render_profile::LEGACY_TRIALS.iter().map(|key|(*key,std::env::var_os(key))).collect::<Vec<_>>();
    let previous=std::env::var_os("TTFX_BG_RENDER_PROFILE");
    for key in crate::render_profile::LEGACY_TRIALS {std::env::set_var(key,"1");}
    for value in [None,Some("reference"),Some("invalid"),Some("clipped"),Some("axis")] {
        if let Some(value)=value{std::env::set_var("TTFX_BG_RENDER_PROFILE",value);}else{std::env::remove_var("TTFX_BG_RENDER_PROFILE");}
        let profile=RenderProfile::parse(value);
        let mut grid=GpuGrid::new(20,20,8.);grid.set_size(320,240,1.);grid.fill(&[]).unwrap();
        let mut pipeline=GpuPipeline::new(true).unwrap();
        gl::Viewport(0,0,320,240);pipeline.draw(&mut grid,true,true,None);
        let status=pipeline.profile_status(&grid);eprintln!("RENDER_PROFILE_GATE {status}");
        assert_eq!(status["profile"],profile.name());assert_eq!(status["backend"],profile.name());assert!(status["fallback"].is_null());
        assert_eq!(pipeline.instanced_cells,profile==RenderProfile::Clipped);
        assert_eq!(pipeline.axis.is_some(),profile==RenderProfile::Axis);
        assert_eq!(status["cache_payload_bytes"],if profile.cached(){4096}else{0});
        assert!(pipeline.cell_upload_cache.is_none());assert!(pipeline.timer.is_none());
        assert_eq!(gl::GetError(),gl::NO_ERROR);
    }
    for(key,value)in old{if let Some(value)=value{std::env::set_var(key,value);}else{std::env::remove_var(key);}}
    if let Some(value)=previous{std::env::set_var("TTFX_BG_RENDER_PROFILE",value);}else{std::env::remove_var("TTFX_BG_RENDER_PROFILE");}
}}

#[test]
#[ignore="axis allocation error response preserves exact reference output and recovers, no per-frame query"]
fn optimized_axis_upload_error_falls_back_and_recovers(){unsafe{
    let _context=Sandbox::new();
    let mut pipeline=GpuPipeline::with_profile(true,RenderProfile::Axis).unwrap();
    let mut reference=GpuPipeline::with_profile(true,RenderProfile::Reference).unwrap();
    let(mut a,mut b)=(GpuGrid::with_profile(20,20,8.,RenderProfile::Reference),GpuGrid::with_profile(20,20,8.,RenderProfile::Axis));
    for(step,(w,fail))in[(320,true),(320,false),(321,false),(321,false)].into_iter().enumerate(){
        let fallback=step<2;let h=240;gl::Viewport(0,0,w,h);
        a.set_size(w,h,1.);b.set_size(w,h,1.);
        let cells=[DirectCell{x:0,y:0,ch:'界',color:(92,64,138),background:Some((255,0,255)),attrs:4}];a.fill(&cells).unwrap();b.fill(&cells).unwrap();
        pipeline.axis.as_mut().unwrap().fail_upload_for_test(fail);
        reference.draw(&mut a,true,false,None);let mut expected=vec![0;w as usize*h as usize*4];gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,expected.as_mut_ptr().cast());
        pipeline.draw(&mut b,true,false,None);let mut got=vec![0;expected.len()];gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,got.as_mut_ptr().cast());
        assert_pixels_equal(&[expected],&[got],"axis upload error/fallback/recovery");
        let status=pipeline.profile_status(&b);assert_eq!(status["backend"],if fallback{"reference"}else{"axis"});assert_eq!(status["fallback"].as_str(),fallback.then_some("axis_upload_gl_error"));
        if step==1 || step==3 {assert_eq!(pipeline.axis.as_ref().unwrap().rebuilds,0);assert_eq!(pipeline.axis.as_ref().unwrap().upload_bytes,0);}
        assert_eq!(gl::GetError(),gl::NO_ERROR);
    }
}}

#[test]
#[ignore="same-context reclamation and foreign-context safety for program, VAO and all textures"]
fn optimized_profile_gl_resources_follow_owner_context(){unsafe{
    let owner=Sandbox::new();
    for profile in [RenderProfile::Reference,RenderProfile::Clipped,RenderProfile::Axis] {
        for _ in 0..3 {
            let mut grid=GpuGrid::with_profile(20,20,8.,profile);grid.set_size(320,240,1.);grid.fill(&[]).unwrap();
            let mut pipeline=GpuPipeline::with_profile(true,profile).unwrap();gl::Viewport(0,0,320,240);pipeline.draw(&mut grid,true,true,None);
            let(program,vao)=(pipeline.program,pipeline.vao);let mut textures=vec![pipeline.fg_tex,pipeline.bg_tex,pipeline.atlas_tex];
            if let Some(axis)=pipeline.axis.as_ref(){textures.extend(axis.textures_for_test());}
            assert_ne!(gl::IsProgram(program),0);assert!(textures.iter().all(|&t|gl::IsTexture(t)!=0));
            let stranger=GpuPipeline::with_profile(true,RenderProfile::Reference).unwrap();let stranger_program=stranger.program;
            drop(pipeline);
            assert_eq!(gl::IsProgram(program),0);assert_eq!(gl::IsVertexArray(vao),0);assert!(textures.iter().all(|&t|gl::IsTexture(t)==0));
            let mut current=0;gl::GetIntegerv(gl::CURRENT_PROGRAM,&mut current);assert_eq!(current as u32,stranger_program);drop(stranger);
        }
    }
    // A context switch must never use object IDs captured from the old context.
    let orphan=GpuPipeline::with_profile(true,RenderProfile::Axis).unwrap();
    let other=eglCreateContext(owner.display,owner.config,std::ptr::null_mut(),[0x3098,3,0x3038].as_ptr());assert!(!other.is_null());
    assert_ne!(eglMakeCurrent(owner.display,owner.surface,owner.surface,other),0);
    let survivor=GpuPipeline::with_profile(true,RenderProfile::Reference).unwrap();let id=survivor.program;
    drop(orphan);assert_ne!(gl::IsProgram(id),0);drop(survivor);
    assert_ne!(eglMakeCurrent(owner.display,owner.surface,owner.surface,owner.context),0);eglDestroyContext(owner.display,other);
    assert_eq!(gl::GetError(),gl::NO_ERROR);
}}

#[test]
#[ignore="optional backend shader failure compiles reference, retains cache, exact output"]
fn optimized_profiles_backend_compile_failure_uses_reference(){unsafe{
    let _context=Sandbox::new();
    let (expected,_)=frames(RenderProfile::Reference);
    FORCE_BACKEND_SHADER_FAILURE.with(|fail|fail.set(true));
    for profile in [RenderProfile::Clipped,RenderProfile::Axis] {
        let(mut grid,mut pipeline)=(GpuGrid::with_profile(20,20,8.,profile),GpuPipeline::with_profile(true,profile).unwrap());
        grid.set_size(320,240,1.);grid.fill(&[]).unwrap();pipeline.draw(&mut grid,true,true,None);
        let status=pipeline.profile_status(&grid);assert_eq!(status["backend"],"reference");assert_eq!(status["fallback"],"backend_initialization_failed");assert_eq!(status["cache_payload_bytes"],4096);
        let (got,_)=frames(profile);assert_pixels_equal(&expected,&got,"shader compilation fallback");
    }
    FORCE_BACKEND_SHADER_FAILURE.with(|fail|fail.set(false));assert_eq!(gl::GetError(),gl::NO_ERROR);
}}
