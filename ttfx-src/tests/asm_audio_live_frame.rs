#![cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use clap::Parser;
use std::ffi::c_void;
use ttfx::asm::{self, AudioHook, DirectFrameHook};
use ttfx::cli::Cli;
use ttfx::engine::effect::RunOutcome;
use ttfx::engine::terminal::TerminalConfig;
use ttfx::utils::rng::Rng;

#[derive(Clone, Debug, PartialEq)]
struct Cell { symbol: u64, fg: u64, bg: u64, attrs: u64 }
#[derive(Default)]
struct Recorder { frames: Vec<Vec<Cell>>, active: bool, no_color: bool, polls: usize }
extern "C" fn poll(ctx: *mut c_void) -> u64 {
    let r=unsafe { &mut *(ctx as *mut Recorder) };r.polls+=1;0
}
fn strength(index: usize) -> Option<f32> { [Some(0.5),Some(1.5),None,Some(1.0)][index%4] }
extern "C" fn params(ctx: *mut c_void, out: *mut f32) -> u64 {
    let r=unsafe { &*(ctx as *mut Recorder) };
    let Some(b)=r.active.then(|| strength(r.frames.len())).flatten() else { return 0; };
    let p=[b,1.,0.,0.,0.,1.,0.,0.,0.,1.];
    unsafe { std::ptr::copy_nonoverlapping(p.as_ptr(),out,p.len()); }1
}
unsafe extern "C" fn emit(ctx: *mut c_void,cols:u64,rows:u64,grid:*const u32,pool:*const u8)->u64 {
    let r=unsafe { &mut *(ctx as *mut Recorder) };
    let handles=unsafe { std::slice::from_raw_parts(grid,(cols*rows) as usize) };
    let cells=handles.iter().map(|&h|{
        assert_ne!(h,0);
        let off=(h&0xffffff) as usize;let len=(h>>24) as usize;
        assert!((32..0xffff00).contains(&off));assert!((1..=128).contains(&len));
        let ptr=unsafe{pool.add(off-32)};
        let word=|o|unsafe{(ptr.add(o) as *const u64).read_unaligned()};
        Cell{symbol:word(0),fg:word(8),bg:word(16),attrs:word(24)}
    }).collect();
    let stats=asm::direct_audio_export_stats().expect("same-thread ASM export observer");
    assert_eq!(stats[0],u64::from(r.active && !r.no_color && strength(r.frames.len()).is_some()));
    assert!(stats[2]<=stats[1]);
    r.frames.push(cells);0
}
fn run(effect:&str,active:bool,no_color:bool)->Recorder {
    assert!(asm::direct_audio_export_stats().is_none(),"observer must reject outside active callback");
    assert!(asm::tier()>0,"must execute real ASM");
    let e=Cli::try_parse_from(["ttfx",effect]).unwrap().effect.unwrap();
    let c=TerminalConfig{canvas_width:40,canvas_height:16,ignore_terminal_dimensions:true,
        frame_rate:60,no_color,..TerminalConfig::default()};
    let mut rng=Rng::seeded(20261004);let mut r=Recorder{active,no_color,..Recorder::default()};
    let ctx=(&mut r as *mut Recorder).cast();
    let got=unsafe{asm::run_direct(asm::Run{effect:&e,input:"ASM REACTIVITY",config:&c,rng:&mut rng,
        parity_dump:false,virtual_clock:true,max_frames:Some(48),tty_output:false,
        audio:Some(AudioHook{poll,params,ctx})},DirectFrameHook{emit,ctx})}.unwrap();
    assert_eq!(got,RunOutcome::Complete);assert_eq!(r.frames.len(),48);assert!(r.polls>=49);r
}
fn tint(word:u64,b:f32)->u64 {
    [16,8,0].into_iter().fold(0,|rgb,shift|rgb | ((((word>>shift)&255) as f32*b).clamp(0.,255.) as u64)<<shift)
}
#[test]
fn cached_and_prebuilt_visuals_follow_current_frame_audio_and_return_to_silence() {
    for effect in ["thunderstorm","rings","decrypt","waves"] {
        let base=run(effect,false,false);let live=run(effect,true,false);let mut colored=0;
        for (frame,(plain,audio)) in base.frames.iter().zip(&live.frames).enumerate() {
            for (cell,(p,a)) in plain.iter().zip(audio).enumerate() {
                assert_eq!((p.symbol,p.attrs),(a.symbol,a.attrs),"geometry/style {effect} frame{frame} cell{cell}");
                let empty=p.symbol==(1u64<<32)|b' ' as u64 && p.fg==u64::MAX && p.bg==u64::MAX;
                let (fg,bg)=if let Some(b)=strength(frame).filter(|_|!empty) {
                    let fg=tint(if p.fg==u64::MAX{0xcccccc}else{p.fg},b);
                    let bg=if p.bg==u64::MAX{p.bg}else{tint(p.bg,b)};
                    if p.fg!=u64::MAX && p.symbol!=(1u64<<32)|b' ' as u64 { colored+=1; }
                    (fg,bg)
                } else { (p.fg,p.bg) };
                assert_eq!((fg,bg),(a.fg,a.bg),"current-frame audio {effect} frame{frame} cell{cell}");
            }
        }
        assert!(colored>0,"must exercise actual colored {effect} output");
        let no_audio_color=run(effect,true,true);let plain=run(effect,false,true);
        assert_eq!(no_audio_color.frames,plain.frames,"--no-color must remain exact {effect}");
    }
}
