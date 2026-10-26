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
        let surface=eglCreatePbufferSurface(display,config,[0x3057,512,0x3056,512,0x3038].as_ptr());
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
unsafe fn frames(upload_cache:bool)->(Vec<Vec<u8>>,Vec<[usize;2]>) {
    if upload_cache {std::env::set_var("TTFX_BG_GPU_UPLOAD_DIRTY","1");}
    else {std::env::remove_var("TTFX_BG_GPU_UPLOAD_DIRTY");}
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
    let(a,base)=frames(false);let(b,changed)=frames(true);
    assert_eq!(a,b,"Píxeles RGBA diferentes: glyph/slotsaltos/alpha/resize/reset");
    assert!(base.iter().all(|x|x[0]>0&&x[1]>0));
    assert_eq!(changed[1][1],0,"BG idéntico no debe enviarse");
    assert_eq!(changed[3][0],0,"FG idéntico no debe enviarse");
    assert_eq!(changed[4],[0,0],"Ninguna textura cambia en step4");
    assert!(changed[6].iter().all(|x|*x>0),"Reset atlas fuerza ambas");
    assert!(changed[7].iter().all(|x|*x>0),"Resize fuerza ambas");
    std::env::remove_var("TTFX_BG_GPU_UPLOAD_DIRTY");
}}
