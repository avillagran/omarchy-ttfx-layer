use super::*;
fn pair()->(GpuGrid,GpuGrid){let(mut a,mut b)=(GpuGrid::new(237,61,9.),GpuGrid::new(237,61,9.));a.slot_cache=None;b.slot_cache=Some(crate::glyph_slot_cache::SlotCache::new());a.set_size(2560,1600,2.);b.set_size(2560,1600,2.);(a,b)}
fn equal(a:&GpuGrid,b:&GpuGrid){assert_eq!(a.fg,b.fg);assert_eq!(a.bg,b.bg);assert_eq!(a.slots,b.slots);assert_eq!(a.pending_masks,b.pending_masks);assert_eq!(a.atlas_reset,b.atlas_reset);}
fn cells(keys:&[(char,u64)],step:usize)->Vec<DirectCell>{keys.iter().enumerate().map(|(i,&(ch,attrs))|DirectCell{x:i%237,y:i/237,ch,color:((i+step)as u8,255-step as u8,77),background:if i%7==0{Some((step as u8,9,99))}else{None},attrs}).collect()}
fn adversarial(n:usize)->Vec<(char,u64)>{use crate::glyph_slot_cache::SlotCache;let bucket=SlotCache::index(SlotCache::key('A',0).unwrap());(1..=0x10ffff).filter_map(char::from_u32).filter(|ch|SlotCache::index(SlotCache::key(*ch,0).unwrap())==bucket).take(n).map(|c|(c,0)).collect()}
#[test]fn general_unicode_styles_collisions_reset_resize_and_slot_order_match(){
    assert_eq!(STYLE_BITS,crate::glyph_slot_cache::STYLE_MASK);
    let(mut a,mut b)=pair();let styles:Vec<_>=(0..128).filter(|s|s&!STYLE_BITS==0).collect();
    for step in 0..24{let keys=if step%3==0{adversarial(24)}else{(0..512).map(|i|(char::from_u32(0x100+i).unwrap(),styles[(i as usize+step)%styles.len()])).collect()};let frame=cells(&keys,step);
        if step%5==0{a.reset_atlas();b.reset_atlas();}if step%7==0{a.set_size(2560-step as i32,1600,2.);b.set_size(2560-step as i32,1600,2.);}
        assert_eq!(a.fill(&frame).is_ok(),b.fill(&frame).is_ok());equal(&a,&b);a.pending_masks.clear();b.pending_masks.clear();
        // Same key after reset gets the newly allocated slot, not the old cache value.
        a.reset_atlas();b.reset_atlas();let reversed:Vec<_>=frame.into_iter().rev().collect();a.fill(&reversed).unwrap();b.fill(&reversed).unwrap();equal(&a,&b);
    }
    // Invalid unnormalized styles bypass cache, retaining original full HashMap keys.
    for style in [8,128,256,u64::MAX]{assert_eq!(a.slot_for('界',style).unwrap(),b.slot_for('界',style).unwrap());equal(&a,&b);}
    a.reset_atlas();b.reset_atlas();a.atlas_cols=2;a.atlas_rows=2;b.atlas_cols=2;b.atlas_rows=2;
    let frame=cells(&[('A',0),('B',0),('C',0),('D',0)],0);assert!(a.fill(&frame).is_err());assert!(b.fill(&frame).is_err());equal(&a,&b);
    let frame=cells(&[('D',0),('C',0)],1);a.fill(&frame).unwrap();b.fill(&frame).unwrap();equal(&a,&b);
}
fn native_frames(limit:usize)->Vec<Vec<DirectCell>>{
    use crate::{Screen,AudioState,INTENSITY_WATCHER,SPEED_WATCHER,IntensityWatcher,SpeedWatcher};
    use std::{sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU64}},time::{Instant,Duration}};use rand::SeedableRng;
    let(slot,gen,stop)=(Arc::new(Mutex::new(vec![])),Arc::new(AtomicU64::new(0)),Arc::new(AtomicBool::new(false)));
    let mut screen=Screen::new_direct(237,61,slot,gen,stop);screen.native_swap=false;screen.native_sgr_cache=false;
    INTENSITY_WATCHER.with(|w|*w.borrow_mut()=IntensityWatcher{last_check:Instant::now()+Duration::from_secs(120),intensity:5});SPEED_WATCHER.with(|w|*w.borrow_mut()=SpeedWatcher{last_check:Instant::now()+Duration::from_secs(120),speed:5});
    crate::native_publication::capture_begin(limit);let mut rng=rand::rngs::StdRng::seed_from_u64(12345);crate::fx_matrix_with_rng(&mut screen,&crate::hardcoded_palette("matrix"),5,&AudioState::start(false),false,false,"matrix",&mut rng,Some(12345)).unwrap();crate::native_publication::capture_finish()
}
fn profile(label:&str,frames:&[Vec<DirectCell>]){
    let(mut a,mut b)=pair();let(mut at,mut bt)=(vec![],vec![]);
    for(i,frame)in frames.iter().enumerate(){let sample=|g:&mut GpuGrid|{let t=crate::producer_probe::thread_cpu_ns();g.fill(frame).unwrap();crate::producer_probe::thread_cpu_ns()-t};let(x,y)=if i%2==0{let x=sample(&mut a);let y=sample(&mut b);(x,y)}else{let y=sample(&mut b);let x=sample(&mut a);(x,y)};at.push(x);bt.push(y);equal(&a,&b);a.pending_masks.clear();b.pending_masks.clear();}
    for(window,lo,hi)in[("startup32",0,32.min(frames.len())),("steady",32.min(frames.len()),frames.len())]{if lo==hi{continue}for(mode,v)in[("A",&at[lo..hi]),("B",&bt[lo..hi])]{let mut v=v.to_vec();v.sort();eprintln!("GLYPH_SLOT_CACHE_CPU {}",serde_json::json!({"case":label,"window":window,"mode":mode,"samples":v.len(),"cpu_p50_ns":v[(v.len()-1)/2],"cpu_p95_ns":v[(v.len()*95).div_ceil(100)-1],"scope":"actual fill CPU, alternating A/B, exact planes/masks/slot oracle; no GPU/resources"}));}}
    let cache=b.slot_cache.as_ref().unwrap();eprintln!("GLYPH_SLOT_CACHE_STATS {}",serde_json::json!({"case":label,"hits":cache.hits,"misses":cache.misses,"payload_bytes":crate::glyph_slot_cache::SlotCache::payload_bytes(),"scope":"test-only counters, table payload not RSS; no counters in normal binary"}));
}
#[test]
#[ignore="opt3 real native fill plus general Unicode/styles/adversarial/multitheme CPU profile"]
fn actual_general_glyph_slot_cache_cpu_profile(){
    profile("native61",&native_frames(128));
    let styles:Vec<_>=(0..128).filter(|s|s&!STYLE_BITS==0).collect();
    for (label,keys) in [("unicode512_styles",(0..512).map(|i|(char::from_u32(0x100+i).unwrap(),styles[i as usize%styles.len()])).collect::<Vec<_>>()),("adversarial24",adversarial(24)),("adversarial512",adversarial(512)),("unicode1536_styles",(0..1536).map(|i|(char::from_u32(0x100+i).unwrap(),styles[i as usize%styles.len()])).collect())]{let frames:Vec<_>=(0..96).map(|step|{let mut f=cells(&keys,step);if step%2!=0{f.reverse();}f}).collect();profile(label,&frames);}
}

#[test]fn combined_profiles_preserve_real_native_publications_and_atlas_order(){
    let frames=native_frames(16);
    let mut grids=[RenderProfile::Reference,RenderProfile::Clipped,RenderProfile::Axis].map(|profile|{let mut grid=GpuGrid::with_profile(237,61,9.,profile);grid.set_size(2560,1600,2.);grid});
    for(i,frame)in frames.iter().enumerate(){
        if i==8{for grid in &mut grids{grid.reset_atlas();}}
        for grid in &mut grids{grid.fill(frame).unwrap();}
        equal(&grids[0],&grids[1]);equal(&grids[0],&grids[2]);
        for grid in &mut grids{grid.pending_masks.clear();}
    }
}
