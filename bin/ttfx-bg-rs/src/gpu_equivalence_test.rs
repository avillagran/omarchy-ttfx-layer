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
struct Sandbox {display:Handle,surface:Handle,context:Handle}
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
        Self{display,surface,context}
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
unsafe fn frames(upload_cache:bool,quads:bool)->(Vec<Vec<u8>>,Vec<[usize;2]>) {
    if upload_cache {std::env::set_var("TTFX_BG_GPU_UPLOAD_DIRTY","1");}
    else {std::env::remove_var("TTFX_BG_GPU_UPLOAD_DIRTY");}
    if quads {std::env::set_var("TTFX_BG_GPU_CELL_QUADS","1");}
    else {std::env::remove_var("TTFX_BG_GPU_CELL_QUADS");}
    let mut pipeline=GpuPipeline::new(true).unwrap();
    let mut grid=GpuGrid::new(20,20,8.);
    let cells:Vec<DirectCell>=(0..400).map(|i|DirectCell{
        x:i%20,y:i/20,ch:char::from_u32(0x100+i as u32).unwrap(),
        color:((i%251)as u8,77,180),background:if i%3==0{Some((19,38,57))}else{None},
        attrs:if i%7==0{4}else{0},
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
        let mut pixels=vec![0;w as usize*h as usize*4];
        gl::ReadPixels(0,0,w,h,gl::RGBA,gl::UNSIGNED_BYTE,pixels.as_mut_ptr().cast());
        assert_eq!(gl::GetError(),gl::NO_ERROR,"GL error step {step}");
        snapshots.push(pixels);
    }
    (snapshots,uploads)
}
#[test]
#[ignore="gate GPU explícito: EGL surfaceless/pbufferRGBA8, ejecutar aislado"]
fn gpu_uploads_match_reference_pixels() {unsafe {
    let _context=Sandbox::new();
    let(a,base)=frames(false,false);let(b,changed)=frames(true,false);
    assert_pixels_equal(&a,&b,"upload/glyph/slotsaltos/alpha/resize/reset");
    assert!(base.iter().all(|x|x[0]>0&&x[1]>0));
    assert_eq!(changed[1][1],0,"BG idéntico no debe enviarse");
    assert_eq!(changed[3][0],0,"FG idéntico no debe enviarse");
    assert_eq!(changed[4],[0,0],"Ninguna textura cambia en step4");
    assert!(changed[6].iter().all(|x|*x>0),"Reset atlas fuerza ambas");
    assert!(changed[7].iter().all(|x|*x>0),"Resize fuerza ambas");
    std::env::remove_var("TTFX_BG_GPU_UPLOAD_DIRTY");
}}

unsafe fn seam_frames(quads:bool,force_fallback:bool)->Vec<Vec<u8>> {
    std::env::remove_var("TTFX_BG_GPU_UPLOAD_DIRTY");
    if quads {std::env::set_var("TTFX_BG_GPU_CELL_QUADS","1");}else{std::env::remove_var("TTFX_BG_GPU_CELL_QUADS");}
    let mut snapshots=vec![];
    for (cols,rows,w,h) in [(237,61,2560,1600),(237,61,509,337),(80,24,17,13),(1,1,97,59),(80,24,511,383)] {
        let mut pipeline=GpuPipeline::new(true).unwrap();
        let mut grid=GpuGrid::new(cols,rows,8.);grid.set_size(w,h,1.);
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
#[ignore="gate GPU explícito: equivalencia RGBA de quads, seams, alpha y slots altos"]
fn gpu_cell_quads_match_reference_pixels() {unsafe {
    let _context=Sandbox::new();
    let(a,_)=frames(false,false);let(b,_)=frames(false,true);
    assert_pixels_equal(&a,&b,"quads/slotsaltos/bgflags/reset/alpha/resize");
    let baseline=seam_frames(false,false);
    assert_pixels_equal(&baseline,&seam_frames(true,false),"quads/seams/subrect/degeneradas/panelreal");
    // Fallback conserva shader de referencia y cubre dimensiones fuera de contrato.
    // Fullscreen no usa scissor aquí: comparar sólo primeros frames, con cobertura completa.
    let fallback=seam_frames(true,true);
    for i in (0..baseline.len()).step_by(3) {assert_pixels_equal(std::slice::from_ref(&baseline[i]),std::slice::from_ref(&fallback[i]),"fallback");}
    std::env::remove_var("TTFX_BG_GPU_CELL_QUADS");
}}
