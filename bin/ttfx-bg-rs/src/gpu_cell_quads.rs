//! Límites de celdas por instancia; fallback si la aritmética i32 no es segura.
pub fn instance_count(cols:i32,rows:i32,width:i32,height:i32)->Option<i32> {
    if cols<=0 || rows<=0 || width<=0 || height<=0 {return None;}
    cols.checked_mul(width)?; rows.checked_mul(height)?;
    cols.checked_mul(rows)
}
pub const VERTEX:&str=r#"
uniform vec2 u_size;
uniform vec2 u_grid;
uniform int u_cells_mode;
flat out ivec2 v_cell;
flat out ivec2 v_origin;
void main() {
    if (u_cells_mode == 0) {
        vec2 v = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
        gl_Position = vec4(v * 2.0 - 1.0, 0.0, 1.0);
        v_cell=ivec2(0);v_origin=ivec2(0);return;
    }
    ivec2 grid=ivec2(u_grid);
    ivec2 size=ivec2(u_size);
    ivec2 cell=ivec2(gl_InstanceID % grid.x, gl_InstanceID / grid.x);
    ivec2 lo=cell*size/grid;
    ivec2 hi=(cell+ivec2(1))*size/grid;
    const ivec2 corners[6]=ivec2[6](ivec2(0,0),ivec2(0,1),ivec2(1,0),
                                  ivec2(1,0),ivec2(0,1),ivec2(1,1));
    ivec2 corner=corners[gl_VertexID];
    vec2 pixel=vec2(lo+corner*(hi-lo));
    vec2 clip=pixel/u_size*2.0-1.0;
    gl_Position=vec4(clip.x,-clip.y,0.0,1.0);
    v_cell=cell;v_origin=lo;
}
"#;
pub fn fragment(reference:&str)->String {
    let start=reference.find("    vec2 px = floor(p);").expect("inicio del mapeo referencia");
    let end=reference.find("    vec4 fg = texelFetch").expect("fin del mapeo referencia");
    let fallback=reference[start..end].replace("ivec2 local =", "local =").replace("ivec2 ic =", "ic =");
    let mapping=format!("    ivec2 local; ivec2 ic;\n    if (u_cells_mode != 0) {{ic=v_cell;local=ivec2(p)-v_origin;}} else {{\n{fallback}    }}\n");
    let mut source=reference.to_string();source.replace_range(start..end,&mapping);
    source.replacen("out vec4 frag;", "uniform int u_cells_mode;\nflat in ivec2 v_cell;\nflat in ivec2 v_origin;\nout vec4 frag;",1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_instance_sizes_fall_back_without_overflow() {
        assert_eq!(instance_count(237,61,2560,1600),Some(14457));
        assert_eq!(instance_count(80,24,17,13),Some(1920));
        assert_eq!(instance_count(0,24,100,100),None);
        assert_eq!(instance_count(100000,100000,1,1),None);
        assert_eq!(instance_count(100000,1,100000,1),None);
    }
    #[test]
    fn integer_cell_bounds_partition_pixels_without_holes_or_overlap() {
        for (size,count) in [(2560,237),(1600,61),(509,237),(17,80),(1,10),(16384,5461)] {
            let mut covered=vec![0;size];
            for cell in 0..count {
                let lo=cell*size/count;let hi=(cell+1)*size/count;
                for pixel in lo..hi {
                    covered[pixel]+=1;
                    assert_eq!(cell,((pixel+1)*count-1)/size);
                    assert!(pixel-lo<=(size+count-1)/count);
                }
            }
            assert!(covered.iter().all(|n|*n==1));
        }
    }
}
