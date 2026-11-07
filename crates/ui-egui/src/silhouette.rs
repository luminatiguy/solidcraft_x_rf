//! Silhouette lines of curved surfaces: the mesh edges where the surface turns from facing the
//! eye to facing away. Wireframe and Hidden line draw them, so a body made only of smooth faces
//! (a revolved circle, a cylinder's wall) shows its outline as a drawing does.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use solidcraft_engine::geom::Mesh;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::render::Camera;

type V3 = solidcore_record::Vec3;

/// An interior mesh edge: its ends and the normals of the two triangles on it.
#[derive(Clone, Copy)]
struct Edge {
    a: V3,
    b: V3,
    n1: V3,
    n2: V3,
}

/// Largest mesh whose adjacency is built (triangles).
const MAX_TRIANGLES: usize = 4_000_000;

thread_local! {
    static CACHE: std::cell::RefCell<HashMap<usize, (Weak<Mesh>, Arc<Vec<Edge>>)>> = std::cell::RefCell::new(HashMap::new());
}

fn key(p: V3) -> [i64; 3] {
    // Welds vertices split at face boundaries (a micrometre grid).
    [p.x, p.y, p.z].map(|c| if c.is_finite() { (c * 1e3).round() as i64 } else { 0 })
}

fn adjacency(mesh: &Mesh) -> Vec<Edge> {
    if mesh.triangles.len() > MAX_TRIANGLES {
        return Vec::new();
    }
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut weld = Vec::with_capacity(mesh.positions.len());
    for p in &mesh.positions {
        let n = ids.len() as u32;
        weld.push(*ids.entry(key(*p)).or_insert(n));
    }
    let mut first: HashMap<(u32, u32), (V3, V3, V3)> = HashMap::new();
    let mut out = Vec::new();
    for t in &mesh.triangles {
        let p = t.map(|i| mesh.positions.get(i as usize).copied());
        let [Some(p0), Some(p1), Some(p2)] = p else { continue };
        let n = (p1 - p0).cross(p2 - p0);
        if n.len2() < 1e-36 {
            continue;
        }
        for (i, j, a, b) in [(t[0], t[1], p0, p1), (t[1], t[2], p1, p2), (t[2], t[0], p2, p0)] {
            let (Some(&wi), Some(&wj)) = (weld.get(i as usize), weld.get(j as usize)) else { continue };
            if wi == wj {
                continue;
            }
            let k = (wi.min(wj), wi.max(wj));
            match first.remove(&k) {
                Some((a0, b0, n0)) => out.push(Edge { a: a0, b: b0, n1: n0, n2: n }),
                None => {
                    first.insert(k, (a, b, n));
                }
            }
        }
    }
    out
}

fn edges(mesh: &Arc<Mesh>) -> Arc<Vec<Edge>> {
    let id = Arc::as_ptr(mesh) as usize;
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((w, e)) = c.get(&id)
            && w.upgrade().is_some_and(|m| Arc::ptr_eq(&m, mesh))
        {
            return e.clone();
        }
        if c.len() > 512 {
            c.retain(|_, (w, _)| w.strong_count() > 0);
        }
        let e = Arc::new(adjacency(mesh));
        c.insert(id, (Arc::downgrade(mesh), e.clone()));
        e
    })
}

/// The silhouette segments of a world-space mesh seen by `cam`.
pub(crate) fn lines(mesh: &Arc<Mesh>, cam: &Camera) -> Vec<(V3, V3)> {
    let eye = cam.eye();
    let back = cam.back();
    let persp = cam.fov > 0.0;
    edges(mesh)
        .iter()
        .filter(|e| {
            let v = if persp { eye - (e.a + e.b) * 0.5 } else { back };
            let (d1, d2) = (e.n1.dot(v), e.n2.dot(v));
            (d1 > 0.0) != (d2 > 0.0)
        })
        .map(|e| (e.a, e.b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cylinder_seen_from_the_side_has_two_silhouette_lines() {
        let mut app = crate::scenario::Harness::new();
        app.app.run("solid.cylinder", serde_json::json!({"radius":5,"height":20})).unwrap();
        let st = crate::open_meshing::shown_world(&app.app);
        let b = st.bodies.first().unwrap();
        let mesh = b.mesh();
        let mut cam = Camera { fov: 0.0, ..Camera::default() };
        cam.set_view(solidcraft_engine::render::StandardView::Front);
        let l = lines(&mesh, &cam);
        // Vertical segments along the wall's two sides.
        let vertical = l.iter().filter(|(a, b)| (a.z - b.z).abs() > 1e-6).count();
        assert!(vertical >= 2, "{vertical} of {}", l.len());
    }
}
