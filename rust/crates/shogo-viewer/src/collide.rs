//! Sphere vs world-triangle slide. Triangles are stored in Bevy Y-up space.
use std::collections::HashMap;

use bevy::prelude::Vec3;

const CELL: f32 = 128.0;

pub struct Collider {
    tris: Vec<[Vec3; 3]>,
    grid: HashMap<(i32, i32, i32), Vec<u32>>,
}

impl Collider {
    pub fn from_tris(tris: Vec<[Vec3; 3]>) -> Self {
        let tris: Vec<[Vec3; 3]> = tris
            .into_iter()
            .filter(|t| {
                t.iter().all(|p| p.is_finite())
                    && (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-4
            })
            .collect();
        let mut grid: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
        for (i, tri) in tris.iter().enumerate() {
            let min = tri[0].min(tri[1]).min(tri[2]);
            let max = tri[0].max(tri[1]).max(tri[2]);
            let (x0, y0, z0) = cell_of(min);
            let (x1, y1, z1) = cell_of(max);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    for z in z0..=z1 {
                        grid.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self { tris, grid }
    }

    /// Push `center` out of the mesh, up to a few iterations. Returns the
    /// corrected center and whether the sphere is resting on an upward face.
    pub fn depenetrate(&self, mut center: Vec3, radius: f32) -> (Vec3, bool) {
        let mut grounded = false;
        for _ in 0..8 {
            let Some((normal, depth)) = self.deepest(center, radius) else {
                break;
            };
            center += normal * (depth + 0.05);
            if normal.y > 0.5 {
                grounded = true;
            }
        }
        (center, grounded)
    }

    /// Highest mostly-horizontal triangle under `(x, z)` whose plane is at or
    /// below `max_y`. Used to stand the player on the floor at a start point.
    pub fn floor_y(&self, x: f32, z: f32, max_y: f32) -> Option<f32> {
        let cx = (x / CELL).floor() as i32;
        let cz = (z / CELL).floor() as i32;
        let mut best: Option<f32> = None;
        for (&(gx, _, gz), ids) in &self.grid {
            if (gx - cx).abs() > 1 || (gz - cz).abs() > 1 {
                continue;
            }
            for &id in ids {
                let tri = self.tris[id as usize];
                let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                let len = n.length();
                if len < 1e-4 || n.y < len * 0.5 {
                    continue;
                }
                let Some(y) = bary_y(tri, x, z) else {
                    continue;
                };
                if !y.is_finite() || y > max_y {
                    continue;
                }
                best = Some(best.map_or(y, |b| b.max(y)));
            }
        }
        best
    }

    /// Nearest triangle hit along a ray. `dir` must be normalized. `None` when
    /// nothing is in front of the origin within `max_dist`.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<f32> {
        let mut best = max_dist;
        let mut hit = false;
        for tri in &self.tris {
            let Some(t) = ray_triangle(origin, dir, *tri) else {
                continue;
            };
            if t > 0.02 && t < best {
                best = t;
                hit = true;
            }
        }
        hit.then_some(best)
    }

    pub fn triangles(&self) -> &[[Vec3; 3]] {
        &self.tris
    }

    /// Rebuild the grid with extra triangles. Used for the arena walls.
    pub fn append(&mut self, extra: Vec<[Vec3; 3]>) {
        let mut tris = std::mem::take(&mut self.tris);
        tris.extend(extra);
        *self = Self::from_tris(tris);
    }

    fn deepest(&self, center: Vec3, radius: f32) -> Option<(Vec3, f32)> {
        let reach = radius + 1.0;
        let (x0, y0, z0) = cell_of(center - Vec3::splat(reach));
        let (x1, y1, z1) = cell_of(center + Vec3::splat(reach));
        let mut best: Option<(Vec3, f32)> = None;
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let Some(ids) = self.grid.get(&(x, y, z)) else {
                        continue;
                    };
                    for &id in ids {
                        let tri = self.tris[id as usize];
                        let q = closest_on_tri(center, tri[0], tri[1], tri[2]);
                        let delta = center - q;
                        let dist = delta.length();
                        if dist >= radius || dist < 1e-5 {
                            continue;
                        }
                        let depth = radius - dist;
                        let normal = delta / dist;
                        if !normal.is_finite() || !depth.is_finite() {
                            continue;
                        }
                        if best.map(|(_, d)| depth > d).unwrap_or(true) {
                            best = Some((normal, depth));
                        }
                    }
                }
            }
        }
        best
    }
}

fn bary_y(tri: [Vec3; 3], x: f32, z: f32) -> Option<f32> {
    let [a, b, c] = tri;
    let den = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
    if den.abs() < 1e-4 {
        return None;
    }
    let w0 = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / den;
    let w1 = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / den;
    let w2 = 1.0 - w0 - w1;
    if w0 < -0.02 || w1 < -0.02 || w2 < -0.02 {
        return None;
    }
    Some(w0 * a.y + w1 * b.y + w2 * c.y)
}

fn ray_triangle(origin: Vec3, dir: Vec3, tri: [Vec3; 3]) -> Option<f32> {
    let [a, b, c] = tri;
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-6 {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = origin - a;
    let u = tvec.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

fn cell_of(p: Vec3) -> (i32, i32, i32) {
    (
        (p.x / CELL).floor() as i32,
        (p.y / CELL).floor() as i32,
        (p.z / CELL).floor() as i32,
    )
}

/// Closest point on triangle abc to p (Ericson).
pub(crate) fn closest_on_tri(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let denom = d1 - d3;
        if denom.abs() < 1e-8 {
            return a;
        }
        let v = d1 / denom;
        return a + ab * v;
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let denom = d2 - d6;
        if denom.abs() < 1e-8 {
            return a;
        }
        let w = d2 / denom;
        return a + ac * w;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let denom = (d4 - d3) + (d5 - d6);
        if denom.abs() < 1e-8 {
            return b;
        }
        let w = (d4 - d3) / denom;
        return b + (c - b) * w;
    }
    let denom_s = va + vb + vc;
    if !denom_s.is_finite() || denom_s.abs() < 1e-8 {
        return a;
    }
    let denom = 1.0 / denom_s;
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raycast_hits_the_triangle_in_front() {
        let col = Collider::from_tris(vec![[
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ]]);
        let hit = col
            .raycast(Vec3::new(0.3, 4.0, 0.3), Vec3::new(0.0, -1.0, 0.0), 10.0)
            .unwrap();
        assert!((hit - 4.0).abs() < 1e-3);
        assert!(col
            .raycast(Vec3::new(5.0, 4.0, 5.0), Vec3::NEG_Y, 10.0)
            .is_none());
    }
}
