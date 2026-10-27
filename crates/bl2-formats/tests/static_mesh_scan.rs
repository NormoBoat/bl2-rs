//! Parses the native data of every StaticMesh and checks invariants (plan/12-tasks.md T-035).
//! Run: $env:BL2_INSTALL="..."; cargo test -p bl2-formats --release --test static_mesh_scan -- --ignored --nocapture

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bl2_formats::package::{Package, Reader};
use bl2_formats::properties::ClassTree;
use bl2_formats::static_mesh::StaticMeshData;

const NOT_PACKAGES: [&str; 2] = ["Mancana_Textures_Data.upk", "Mancana_Textures_Meta.upk"];

fn upk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            upk_files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk"))
            && !NOT_PACKAGES.contains(&p.file_name().unwrap().to_string_lossy().as_ref())
        {
            out.push(p);
        }
    }
}

#[test]
#[ignore = "needs the user's Borderlands 2 install in BL2_INSTALL"]
fn every_static_mesh_parses() {
    let root = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let mut files = Vec::new();
    upk_files(&root, &mut files);
    files.sort();
    let mut tree = ClassTree::default();
    for f in files.iter().filter(|f| f.with_extension("upk.uncompressed_size").exists()) {
        tree.add_package(&Package::open(f).unwrap());
    }
    tree.add_editor_classes();

    let (mut total, mut ok) = (0usize, 0usize);
    let mut errors: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let p = Package::open(f).unwrap();
        for e in p.exports.iter().filter(|e| e.class != 0 && e.object_flags & 0x200 == 0 && p.object_name(e.class) == "StaticMesh") {
            total += 1;
            let end = e.serial_offset + e.serial_size;
            let mut r = Reader::new(&p.data[..end], e.serial_offset);
            let res = p
                .read_object_header(e, &tree, &mut r)
                .and_then(|_| p.read_tags(&mut r))
                .and_then(|_| StaticMeshData::read(&mut r))
                .map_err(|x| x.to_string())
                .and_then(|m| {
                    if r.pos != end {
                        return Err(format!("{} bytes left (lods={})", end as i64 - r.pos as i64, m.lod_count));
                    }
                    *hist.entry(format!("after_lods={}", m.unknown_after_lods)).or_default() += 1;
                    *hist.entry(format!("unknown_9={:02x?}", m.unknown_9)).or_default() += 1;
                    *hist.entry(format!("trailing={} vpv={} factors={}", m.trailing, m.vertex_position_version, m.cached_streaming_texture_factors.len())).or_default() += 1;
                    if total == 1 {
                        println!("FIRST {} {}: {m:?}", f.file_name().unwrap().to_string_lossy(), p.name(e.object_name));
                    }
                    *hist.entry(format!("internal_version={}", m.internal_version)).or_default() += 1;
                    *hist.entry(format!("unknown_12_zero={}", m.unknown_12 == [0; 12])).or_default() += 1;
                    *hist.entry(format!("lod_count={}", m.lod_count)).or_default() += 1;
                    *hist.entry(format!("kdop_node_size={}", m.kdop_node_size)).or_default() += 1;
                    for (lod_index, lod) in m.lods.iter().enumerate() {
                        let b = &lod.buffers;
                        let nv = b.num_vertices as usize;
                        *hist.entry(format!("uv_stride={} tex_coords={} full={}", b.uv_stride, b.num_tex_coords, b.full_precision_uvs)).or_default() += 1;
                        *hist.entry(format!("color_stride={} has_colors={}", b.color_stride, b.colors.is_some())).or_default() += 1;
                        if b.positions.element_size != b.position_stride as usize || b.positions.count != nv {
                            return Err(format!("positions {:?} vs stride {} verts {nv}", b.positions, b.position_stride));
                        }
                        if b.tangents_uvs.element_size != b.uv_stride as usize || b.tangents_uvs.count != nv {
                            return Err(format!("tangents/uvs {:?} vs stride {} verts {nv}", b.tangents_uvs, b.uv_stride));
                        }
                        if let Some(c) = &b.colors {
                            if c.count < nv || c.element_size != b.color_stride as usize {
                                return Err(format!("color buffer {c:?} stride {} vs {nv} vertices", b.color_stride));
                            }
                            *hist.entry(format!("colors_per_vertex={}", c.count / nv.max(1))).or_default() += 1;
                        }
                        if b.indices.element_size != 2 {
                            return Err(format!("index size {}", b.indices.element_size));
                        }
                        // LOD 0 positions lie inside Bounds (origin +- extent) up to float rounding; Bounds come
                        // from LOD 0, so simplified lower LODs may stick out slightly (seen up to 0.3 uu, E-059).
                        let (o, x) = (m.bounds.origin, m.bounds.box_extent);
                        let max_extent = x.iter().fold(0f32, |a, b| a.max(b.abs()));
                        for v in p.data[b.positions.data.clone()].as_chunks::<12>().0 {
                            for a in 0..3 {
                                let c = f32::from_le_bytes(v[a * 4..a * 4 + 4].try_into().unwrap());
                                let lod_slack = if lod_index == 0 { 0.0 } else { 0.05 * max_extent };
                                let tol = 1e-3 * (1.0 + x[a].abs() + o[a].abs()) + lod_slack;
                                if !(c >= o[a] - x[a] - tol && c <= o[a] + x[a] + tol) {
                                    return Err(format!("LOD {lod_index} axis {a}: vertex coordinate {c} outside bounds {o:?} +- {x:?}"));
                                }
                            }
                        }
                        let idx = &p.data[b.indices.data.clone()];
                        if idx.as_chunks::<2>().0.iter().any(|c| u16::from_le_bytes(*c) as usize >= nv) {
                            return Err("index out of vertex range".into());
                        }
                        for el in &lod.elements {
                            *hist.entry(format!("element_unknown_byte={}", el.unknown_byte)).or_default() += 1;
                            let frag_tris: u32 = el.fragments.iter().map(|f| f.num_primitives).sum();
                            if frag_tris != el.num_triangles {
                                return Err(format!("fragments cover {frag_tris} of {} triangles", el.num_triangles));
                            }
                            if (el.first_index + el.num_triangles * 3) as usize > b.indices.count {
                                return Err("element triangles past index buffer".into());
                            }
                        }
                    }
                    Ok(())
                });
            match res {
                Ok(()) => ok += 1,
                Err(err) => {
                    let key: String = err.chars().take(40).collect();
                    let entry = errors.entry(key).or_insert((0, format!("{} {}: {err}", f.file_name().unwrap().to_string_lossy(), p.name(e.object_name))));
                    entry.0 += 1;
                }
            }
        }
    }
    println!("static_meshes={total} ok={ok}");
    println!("HIST {hist:?}");
    for (n, ex) in errors.values() {
        println!("ERR x{n}: {ex}");
    }
    assert_eq!(ok, total);
}
