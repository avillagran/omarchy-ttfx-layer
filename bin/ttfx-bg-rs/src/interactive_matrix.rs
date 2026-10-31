//! Opt-in desktop rain: a read-only Hyprland cursor feed selects extra rain lanes.
//! No input capture or branding; system-playback audio drives the new simulation.
use std::{sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Duration, Instant}};
use rand::{Rng, SeedableRng};
use crate::{asm_direct_bridge::DirectCell, frame_wakeup::FrameWakeup};

#[derive(Default)]
pub struct PointerState(Mutex<Option<(f32, f32)>>);
impl PointerState {
    pub fn update(&self, x: f64, y: f64, width: u32, height: u32) {
        let point = if x.is_finite() && y.is_finite() && width > 0 && height > 0 {
            Some(((x / width as f64).clamp(0., 1.) as f32, (y / height as f64).clamp(0., 1.) as f32))
        } else { None };
        *self.0.lock().unwrap() = point;
    }
    pub fn leave(&self) { *self.0.lock().unwrap() = None; }
    pub(crate) fn sample(&self) -> Option<(f32, f32)> { *self.0.lock().unwrap() }
}
// A single bounded IPC read per sample, without spawning hyprctl processes.
fn query_hypr(socket: &std::path::Path, command: &str) -> anyhow::Result<serde_json::Value> {
    use std::io::{Read, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_millis(20)))?;
    stream.set_write_timeout(Some(Duration::from_millis(20)))?;
    stream.write_all(command.as_bytes())?;
    let mut bytes=Vec::new();
    stream.take(65536).read_to_end(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
#[derive(Clone, Copy)]
struct OutputRect { x:f64, y:f64, width:f64, height:f64 }
impl OutputRect {
    fn from_monitor(m:&serde_json::Value) -> Option<Self> {
        let scale=m["scale"].as_f64()?;
        if !scale.is_finite() || scale<=0. { return None; }
        let (mut width,mut height)=(m["width"].as_f64()?/scale,m["height"].as_f64()?/scale);
        if m["transform"].as_u64().unwrap_or(0)%2==1 { std::mem::swap(&mut width,&mut height); }
        if width<=0. || height<=0. { return None; }
        Some(Self{x:m["x"].as_f64()?,y:m["y"].as_f64()?,width,height})
    }
    fn feed(self,pointer:&PointerState,x:f64,y:f64) {
        if x>=self.x && y>=self.y && x<self.x+self.width && y<self.y+self.height {
            pointer.update((x-self.x)/self.width,(y-self.y)/self.height,1,1);
        } else { pointer.leave(); }
    }
}
pub fn track_global(pointer:Arc<PointerState>, stop:Arc<AtomicBool>, output:Option<&str>, follow_outside:bool) -> anyhow::Result<std::thread::JoinHandle<()>> {
    let socket=std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?)
        .join("hypr").join(std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?).join(".socket.sock");
    let output=output.map(str::to_owned);
    let read_rect=|socket:&std::path::Path| -> anyhow::Result<OutputRect> {
        let monitors=query_hypr(socket,"j/monitors")?;
        monitors.as_array().and_then(|ms|ms.iter().find(|m|output.as_ref().is_none_or(|o|m["name"]==*o)))
            .and_then(OutputRect::from_monitor).ok_or_else(||anyhow::anyhow!("interactive Matrix output geometry unavailable"))
    };
    let mut rect=read_rect(&socket)?;
    Ok(std::thread::Builder::new().name("matrix-pointer".into()).spawn(move|| {
        eprintln!("interactive background: global Hyprland pointer active; click-through, 30 Hz");
        let mut geometry_at=Instant::now();
        while !stop.load(Ordering::Acquire) {
            let begin=Instant::now();
            if geometry_at.elapsed()>=Duration::from_secs(2) {
                if let Ok(monitors)=query_hypr(&socket,"j/monitors") {
                    if let Some(next)=monitors.as_array().and_then(|ms|ms.iter().find(|m|output.as_ref().is_none_or(|o|m["name"]==*o))).and_then(OutputRect::from_monitor) { rect=next; }
                }
                geometry_at=Instant::now();
            }
            match query_hypr(&socket,"j/cursorpos") {
                Ok(p)=>match (p["x"].as_f64(),p["y"].as_f64()) { (Some(x),Some(y))=>if follow_outside { pointer.update((x-rect.x)/rect.width,(y-rect.y)/rect.height,1,1); } else { rect.feed(&pointer,x,y); }, _=>pointer.leave() },
                Err(_)=>pointer.leave(),
            }
            if let Some(rest)=Duration::from_millis(33).checked_sub(begin.elapsed()) { std::thread::sleep(rest); }
        }
        pointer.leave();
    })?)
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Sound { pub bass:f32, pub treble:f32 }
impl Sound {
    pub(crate) fn sample(state:&crate::AudioState) -> Self {
        let gate=(state.volume()*10.).clamp(0.,1.);
        let bass=(0..crate::NBANDS/3).map(|b|state.band_at(b,crate::NBANDS)).sum::<f32>()/(crate::NBANDS/3) as f32;
        Self{bass:(bass*gate).clamp(0.,1.),treble:(state.treble()*gate).clamp(0.,1.)}
    }
    pub(crate) fn follow(&mut self,target:Self,dt:f32) {
        let mix=|old:f32,new:f32|old+(new-old)*(1.-(-dt.clamp(0.,0.1)*if new>old {24.} else {8.}).exp());
        self.bass=mix(self.bass,target.bass);self.treble=mix(self.treble,target.treble);
    }
}
struct Drop { head: f32, speed: f32, trail: usize, birth_light:Option<f32> }
struct Spark { x: usize, head: f32, speed: f32, trail: usize, birth_light:Option<f32> }
pub(crate) fn reactive_motion() -> bool {
    let configured=std::env::var("TTFX_BG_MATRIX_MOTION").ok().or_else(|| {
        let path=std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config/omarchy/ambient/matrix-reactive.conf");
        std::fs::read_to_string(path).ok()?.lines().filter_map(|line|line.trim().split_once('=')).filter(|(k,_)|k.trim()=="motion").map(|(_,v)|v.trim().to_owned()).last()
    });
    configured.as_deref()!=Some("steady")
}
pub struct Rain {
    cols: usize, rows: usize, _aspect: f32,
    pub(crate) reactive_speed: bool,
    drops: Vec<Drop>, glyphs: Vec<char>, chars: Vec<char>, heat: Vec<f32>,
    sparks: Vec<Spark>, spawn_budget: f32, bass_budget:f32,
}
impl Rain {
    pub fn new(cols: usize, rows: usize, aspect: f32, rng: &mut impl Rng) -> Self {
        let chars: Vec<char> = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ0123456789ABCDEF".chars().collect();
        let drops = (0..cols).map(|_| {let head=rng.gen_range(-(rows as f32)..rows as f32);Drop {head,speed:rng.gen_range(12.0..32.0),trail:rng.gen_range(10..25),birth_light:if head>=0. {Some(0.)} else {None}}}).collect();
        let glyphs = (0..cols*rows).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
        Self { cols, rows, reactive_speed:false, _aspect: aspect.clamp(1.,4.), drops, glyphs, chars, heat: vec![0.;cols*rows], sparks: Vec::with_capacity(384), spawn_budget: 0., bass_budget:0. }
    }
    pub fn step(&mut self, dt: f32, pointer: Option<(f32,f32)>, sound: Sound, rng: &mut impl Rng, cells: &mut Vec<DirectCell>) {
        let dt = dt.clamp(0., 0.1);
        let bass=sound.bass.clamp(0.,1.);
        let treble=sound.treble.clamp(0.,1.);
        let motion=if self.reactive_speed {1.+bass*0.4} else {1.};
        let decay = (-dt*3.5).exp();
        for h in &mut self.heat { *h *= decay; }
        if let Some((nx,_ny)) = pointer.filter(|(x,y)| x.is_finite() && y.is_finite()) {
            let px = nx.clamp(0.,1.)*(self.cols-1) as f32;
            // The pointer selects rain lanes, not a radial emitter at its position.
            let radius = 12.;
            let x0 = (px-radius).max(0.) as usize;
            let x1 = ((px+radius) as usize+1).min(self.cols);
            for x in x0..x1 {
                let light = (1.-((x as f32-px)/radius).abs()).max(0.)*0.35;
                for y in 0..self.rows {
                    self.heat[y*self.cols+x] = self.heat[y*self.cols+x].max(light);
                }
            }
            self.spawn_budget = (self.spawn_budget+dt*36.).min(4.);
            while self.spawn_budget >= 1. && self.sparks.len()<384 {
                self.spawn_budget-=1.;
                self.sparks.push(Spark {
                    x: (px+rng.gen_range(-12.0..12.0)).clamp(0.,(self.cols-1) as f32) as usize,
                    head: -rng.gen_range(0.0..1.0),
                    speed: rng.gen_range(12.0..32.0), trail:rng.gen_range(10..25),birth_light:None,
                });
            }
        } else { self.spawn_budget=0.; }
        // Bass adds top-origin streams across the whole output, independently of the mouse.
        self.bass_budget=if bass<0.001 {0.} else {(self.bass_budget+dt*bass*144.).min(16.)};
        while self.bass_budget>=1. && self.sparks.len()<384 {
            self.bass_budget-=1.;
            self.sparks.push(Spark{x:rng.gen_range(0..self.cols),head:-rng.gen_range(0.0..1.0),speed:rng.gen_range(12.0..32.0),trail:rng.gen_range(10..25),birth_light:None});
        }
        cells.clear();
        for (x,drop) in self.drops.iter_mut().enumerate() {
            drop.head += drop.speed*dt*motion;
            if drop.birth_light.is_none() && drop.head>=0. {drop.birth_light=Some(treble);}
            if drop.head-drop.trail as f32 > self.rows as f32 {
                drop.head = -rng.gen_range(5.0..self.rows.max(6) as f32);
                drop.speed = rng.gen_range(12.0..32.0);
                drop.trail = rng.gen_range(10..25);
                drop.birth_light=None;
            }
            for t in 0..drop.trail {
                let y=drop.head.floor() as i32-t as i32;
                if y<0 || y>=self.rows as i32 { continue; }
                let y=y as usize; let i=y*self.cols+x;
                if rng.gen::<f32>() < 0.025 { self.glyphs[i]=self.chars[rng.gen_range(0..self.chars.len())]; }
                let color=strip_color(t,drop.trail,drop.birth_light.unwrap_or(0.),self.heat[i]);
                cells.push(DirectCell { x,y,ch:self.glyphs[i],color,background:None,attrs:0 });
            }
        }
        for spark in &mut self.sparks {
            spark.head+=spark.speed*dt*motion;
            if spark.birth_light.is_none() && spark.head>=0. {spark.birth_light=Some(treble);}
            for t in 0..spark.trail {
                let y=spark.head.floor() as i32-t as i32;
                if y>=0 && y<self.rows as i32 {
                    let y=y as usize; let i=y*self.cols+spark.x;
                    let color=strip_color(t,spark.trail,spark.birth_light.unwrap_or(0.),self.heat[i]);
                    cells.push(DirectCell { x:spark.x,y,ch:self.glyphs[i],color,background:None,attrs:0 });
                }
            }
        }
        self.sparks.retain(|s|s.head-(s.trail as f32)<self.rows as f32);

    }
}
fn strip_color(t:usize,trail:usize,birth:f32,heat:f32)->(u8,u8,u8) {
    let g=20.+180.*(1.-t as f32/trail as f32).powf(1.4);
    let (base,shade)=if t==0 {((110.,255.,165.),1.)} else {((0.,g,g*0.18),g/200.)};
    let light=birth.clamp(0.,1.);
    let boost=1.+heat*0.7;
    let channel=|old:f32,target:f32|((old+(target*shade-old)*light)*boost).clamp(0.,255.) as u8;
    (channel(base.0,225.),channel(base.1,255.),channel(base.2,235.))
}
pub fn run(cols:usize, rows:usize, aspect:f32, pointer:Arc<PointerState>, snapshot:Arc<Mutex<Vec<DirectCell>>>, generation:Arc<AtomicU64>, stop:Arc<AtomicBool>, wakeup:Arc<FrameWakeup>) -> anyhow::Result<()> {
    let sound=crate::AudioState::start(true);
    let mut drive=Sound::default(); let mut sound_seen=false;
    let mut rng=rand::rngs::StdRng::from_entropy(); let mut rain=Rain::new(cols,rows,aspect,&mut rng);
    rain.reactive_speed=reactive_motion();
    let mut cells=Vec::with_capacity(cols*28+384); let mut last=Instant::now();
    while !stop.load(Ordering::Acquire) {
        let begin=Instant::now(); let dt=(begin-last).as_secs_f32(); last=begin;
        crate::fps_governor::note_tick();
        let target=Sound::sample(&sound);drive.follow(target,dt);
        if !sound_seen && (target.bass>0.03 || target.treble>0.03) {eprintln!("interactive Matrix: spectral audio received (bass density / treble birth color)");sound_seen=true;}
        rain.step(dt,pointer.sample(),drive,&mut rng,&mut cells);
        { let mut slot=snapshot.lock().unwrap(); if stop.load(Ordering::Acquire) { break; }
          std::mem::swap(&mut *slot,&mut cells); generation.fetch_add(1,Ordering::Release); wakeup.notify()?; }
        let period=Duration::from_secs_f64(1./crate::max_fps().unwrap_or(60.).clamp(1.,60.));
        if let Some(rest)=period.checked_sub(begin.elapsed()) { std::thread::sleep(rest); }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_is_normalized_bounded_and_cleared_on_leave() {
        let p=PointerState::default(); p.update(50.,25.,100,100); assert_eq!(p.sample(),Some((0.5,0.25)));
        p.update(-10.,200.,100,100); assert_eq!(p.sample(),Some((0.,1.)));
        p.update(f64::NAN,0.,100,100); assert_eq!(p.sample(),None);
        p.leave(); assert_eq!(p.sample(),None);
    }
    #[test]
    fn global_pointer_maps_scaled_negative_outputs_and_leaves_other_monitors() {
        let p=PointerState::default();
        let r=OutputRect::from_monitor(&serde_json::json!({"x":-1000,"y":-200,"width":2000,"height":1200,"scale":2,"transform":0})).unwrap();
        r.feed(&p,-500.,100.); assert_eq!(p.sample(),Some((0.5,0.5)));
        r.feed(&p,10.,100.); assert_eq!(p.sample(),None);
        r.feed(&p,f64::NAN,100.); assert_eq!(p.sample(),None);
        let rotated=OutputRect::from_monitor(&serde_json::json!({"x":0,"y":0,"width":2000,"height":1200,"scale":2,"transform":1})).unwrap();
        assert_eq!((rotated.width,rotated.height),(600.,1000.));
    }
    #[test]
    fn reactive_motion_is_a_bounded_bass_impulse_and_returns_to_base_speed() {
        let mut ar=rand::rngs::StdRng::seed_from_u64(5);let mut br=ar.clone();
        let mut a=Rain::new(80,93,2.,&mut ar);let mut b=Rain::new(80,93,2.,&mut br);
        a.drops[0]=Drop{head:5.,speed:20.,trail:12,birth_light:Some(0.)};
        b.drops[0]=Drop{head:5.,speed:20.,trail:12,birth_light:Some(0.)};b.reactive_speed=true;
        let mut cells=vec![];
        a.step(0.1,None,Sound{bass:1.,treble:0.},&mut ar,&mut cells);
        b.step(0.1,None,Sound{bass:1.,treble:0.},&mut br,&mut cells);
        assert!((a.drops[0].head-7.).abs()<0.001);assert!((b.drops[0].head-7.8).abs()<0.001);
        let before=b.drops[0].head;b.step(0.1,None,Sound::default(),&mut br,&mut cells);
        assert!((b.drops[0].head-before-2.).abs()<0.001);
    }
    #[test]
    fn treble_is_latched_at_visible_birth_for_the_entire_strip() {
        let mut rng=rand::rngs::StdRng::seed_from_u64(9);let mut rain=Rain::new(80,60,2.,&mut rng);let mut cells=vec![];
        rain.drops[0]=Drop{head:-0.1,speed:20.,trail:12,birth_light:None};
        rain.drops[1]=Drop{head:5.,speed:20.,trail:12,birth_light:Some(0.)};
        rain.sparks.push(Spark{x:2,head:-0.1,speed:20.,trail:12,birth_light:None});
        rain.step(0.05,None,Sound{bass:0.,treble:1.},&mut rng,&mut cells);
        assert_eq!(rain.drops[0].birth_light,Some(1.));assert_eq!(rain.drops[1].birth_light,Some(0.));assert_eq!(rain.sparks[0].birth_light,Some(1.));
        let bright=strip_color(0,12,1.,0.);assert!(bright.0>strip_color(0,12,0.,0.).0);
        for _ in 0..10 {rain.step(0.05,None,Sound::default(),&mut rng,&mut cells);}
        assert_eq!(rain.drops[0].birth_light,Some(1.));assert_eq!(rain.drops[1].birth_light,Some(0.));assert_eq!(rain.sparks[0].birth_light,Some(1.));
        let head=rain.drops[0].head.floor() as usize;
        assert_eq!(cells.iter().find(|c|c.x==0&&c.y==head).unwrap().color,bright);
    }
    #[test]
    fn bass_adds_global_streams_proportionally_without_changing_fall_speed() {
        let mut ar=rand::rngs::StdRng::seed_from_u64(12);let mut br=ar.clone();let mut cr=ar.clone();
        let mut a=Rain::new(160,93,2.,&mut ar);let mut b=Rain::new(160,93,2.,&mut br);let mut c=Rain::new(160,93,2.,&mut cr);let mut cells=vec![];
        for _ in 0..30 {a.step(0.03,None,Sound::default(),&mut ar,&mut cells);b.step(0.03,None,Sound{bass:0.25,treble:0.},&mut br,&mut cells);c.step(0.03,None,Sound{bass:1.,treble:0.},&mut cr,&mut cells);}
        assert!(a.sparks.is_empty());assert!(c.sparks.len()>b.sparks.len()*3);
        assert!(c.sparks.iter().any(|s|s.x<40)&&c.sparks.iter().any(|s|s.x>120));
        assert!(c.sparks.iter().filter_map(|s|s.birth_light).all(|x|x==0.));
        assert_eq!(a.drops[0].head,b.drops[0].head);assert_eq!(a.drops[0].head,c.drops[0].head);
    }
    #[test]
    fn pointer_increases_visible_rain_density_not_only_duplicate_cells() {
        use std::collections::HashSet;
        let mut a_rng=rand::rngs::StdRng::seed_from_u64(71);
        let mut b_rng=rand::rngs::StdRng::seed_from_u64(71);
        let mut a=Rain::new(160,93,2.,&mut a_rng);
        let mut b=Rain::new(160,93,2.,&mut b_rng);
        let(mut ac,mut bc)=(vec![],vec![]);
        let(mut baseline,mut reactive)=(0usize,0usize);
        for frame in 0..240 {
            a.step(1./30.,None,Sound::default(),&mut a_rng,&mut ac);
            b.step(1./30.,Some((0.5,0.5)),Sound::default(),&mut b_rng,&mut bc);
            if frame>=90 {
                let count=|cells:&[DirectCell]|cells.iter().filter(|c|(68..92).contains(&c.x)).map(|c|(c.x,c.y)).collect::<HashSet<_>>().len();
                baseline+=count(&ac); reactive+=count(&bc);
            }
        }
        assert!(reactive*100>baseline*150,"visible density: baseline={baseline}, reactive={reactive}");
    }
    #[test]
    fn heat_is_local_fades_and_extra_flow_is_bounded() {
        let mut rng=rand::rngs::StdRng::seed_from_u64(42); let mut rain=Rain::new(160,60,2.,&mut rng); let mut cells=vec![];
        rain.step(0.15,Some((0.5,0.5)),Sound::default(),&mut rng,&mut cells);
        assert!(rain.heat[30*160+80]>0.3); assert_eq!(rain.heat[0],0.);
        rain.step(0.1,Some((0.5,0.5)),Sound::default(),&mut rng,&mut cells);
        assert!(rain.sparks.len()>0);
        assert!(rain.sparks.iter().all(|s|s.head<4. && (10..25).contains(&s.trail)));
        let h=rain.heat[30*160+80];
        for _ in 0..200 { rain.step(0.05,None,Sound::default(),&mut rng,&mut cells); }
        assert!(rain.heat[30*160+80]<h*0.01); assert!(rain.sparks.is_empty());
        for _ in 0..1000 { rain.step(0.1,Some((1.,1.)),Sound::default(),&mut rng,&mut cells); }
        assert!(rain.sparks.len()<=384);
        assert!(cells.iter().all(|c|c.x<160 && c.y<60 && rain.chars.contains(&c.ch)));
    }
}
