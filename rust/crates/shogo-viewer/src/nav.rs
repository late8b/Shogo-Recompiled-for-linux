//! Walk grid baked from the world's floor and wall triangles.
//!
//! Cells are walkable when a mostly-horizontal triangle covers them and no
//! wall triangle crosses the body column. Enemies path with A*.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::prelude::{Resource, Vec3};

use crate::collide::Collider;

#[derive(Resource)]
pub struct WalkNav(pub NavMesh);

#[derive(Clone)]
pub struct NavMesh {
    origin: (f32, f32),
    cell: f32,
    w: i32,
    h: i32,
    floors: Vec<Option<f32>>,
}

impl NavMesh {
    pub fn bake(col: &Collider, center: Vec3, half: f32, cell: f32) -> Self {
        let cell = cell.max(16.0);
        let n = ((half * 2.0) / cell).ceil().max(1.0) as i32;
        let origin_x = center.x - n as f32 * cell * 0.5;
        let origin_z = center.z - n as f32 * cell * 0.5;
        let mut floors: Vec<Option<f32>> = vec![None; (n * n) as usize];
        let hint = center.y;

        for tri in col.triangles() {
            let nrm = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
            let len = nrm.length();
            if len < 1e-4 || nrm.y < len * 0.62 {
                continue;
            }
            let Some((x0, z0, x1, z1)) = tri_cell_span(*tri, origin_x, origin_z, cell, n) else {
                continue;
            };
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let cx = origin_x + (x as f32 + 0.5) * cell;
                    let cz = origin_z + (z as f32 + 0.5) * cell;
                    let Some(y) = bary_y(*tri, cx, cz) else {
                        continue;
                    };
                    let i = (z * n + x) as usize;
                    floors[i] = Some(match floors[i] {
                        Some(old) if (old - hint).abs() <= (y - hint).abs() => old,
                        _ => y,
                    });
                }
            }
        }

        for tri in col.triangles() {
            if !wall_column(*tri) {
                continue;
            }
            let Some((x0, z0, x1, z1)) = tri_cell_span(*tri, origin_x, origin_z, cell, n) else {
                continue;
            };
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let i = (z * n + x) as usize;
                    let Some(floor) = floors[i] else {
                        continue;
                    };
                    if !column_overlap(*tri, floor) {
                        continue;
                    }
                    let min = bevy::prelude::Vec2::new(
                        origin_x + x as f32 * cell,
                        origin_z + z as f32 * cell,
                    );
                    let max = bevy::prelude::Vec2::new(min.x + cell, min.y + cell);
                    if tri_overlaps_rect(*tri, min, max) {
                        floors[i] = None;
                    }
                }
            }
        }

        Self {
            origin: (origin_x, origin_z),
            cell,
            w: n,
            h: n,
            floors,
        }
    }

    pub fn walkable_count(&self) -> usize {
        self.floors.iter().filter(|c| c.is_some()).count()
    }

    pub fn find(&self, from: Vec3, to: Vec3) -> Vec<Vec3> {
        let Some(start) = self.nearest(from, 8) else {
            return Vec::new();
        };
        let Some(goal) = self.nearest(to, 8) else {
            return Vec::new();
        };
        if start == goal {
            return vec![self.point(goal)];
        }
        let n = self.floors.len();
        let mut g = vec![i32::MAX; n];
        let mut parent = vec![None; n];
        let mut heap = BinaryHeap::new();
        g[start] = 0;
        heap.push(Reverse((0, 0, start)));
        let mut found = false;
        while let Some(Reverse((_, g_here, i))) = heap.pop() {
            if g_here != g[i] {
                continue;
            }
            if i == goal {
                found = true;
                break;
            }
            let (ix, iz) = self.coords(i);
            let fy = self.floors[i].unwrap_or(0.0);
            for (dx, dz) in NEIGHBORS {
                let nx = ix + dx;
                let nz = iz + dz;
                let Some(j) = self.index(nx, nz) else {
                    continue;
                };
                let Some(ny) = self.floors[j] else {
                    continue;
                };
                if (ny - fy).abs() > 64.0 {
                    continue;
                }
                if dx != 0 && dz != 0 {
                    let a = self.index(ix + dx, iz);
                    let b = self.index(ix, iz + dz);
                    if a.and_then(|k| self.floors[k]).is_none() || b.and_then(|k| self.floors[k]).is_none()
                    {
                        continue;
                    }
                }
                let step = if dx != 0 && dz != 0 { 141 } else { 100 };
                let climb = ((ny - fy).abs() * 0.4) as i32;
                let next = g_here.saturating_add(step + climb);
                if next < g[j] {
                    g[j] = next;
                    parent[j] = Some(i);
                    let h = self.octile(j, goal);
                    heap.push(Reverse((next.saturating_add(h), next, j)));
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut path = Vec::new();
        let mut i = goal;
        for _ in 0..n {
            path.push(self.point(i));
            match parent[i] {
                Some(p) if p != i => i = p,
                _ => break,
            }
        }
        path.reverse();
        path
    }

    fn nearest(&self, p: Vec3, radius: i32) -> Option<usize> {
        let ix = ((p.x - self.origin.0) / self.cell).floor() as i32;
        let iz = ((p.z - self.origin.1) / self.cell).floor() as i32;
        if let Some(i) = self.index(ix, iz) {
            if self.floors[i].is_some() {
                return Some(i);
            }
        }
        for r in 1..=radius {
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    if let Some(i) = self.index(ix + dx, iz + dz) {
                        if self.floors[i].is_some() {
                            return Some(i);
                        }
                    }
                }
            }
        }
        None
    }

    fn point(&self, i: usize) -> Vec3 {
        let (x, z) = self.coords(i);
        let y = self.floors[i].unwrap_or(0.0);
        Vec3::new(
            self.origin.0 + (x as f32 + 0.5) * self.cell,
            y,
            self.origin.1 + (z as f32 + 0.5) * self.cell,
        )
    }

    fn coords(&self, i: usize) -> (i32, i32) {
        ((i as i32) % self.w, (i as i32) / self.w)
    }

    fn index(&self, x: i32, z: i32) -> Option<usize> {
        if x < 0 || z < 0 || x >= self.w || z >= self.h {
            None
        } else {
            Some((z * self.w + x) as usize)
        }
    }

    fn octile(&self, a: usize, b: usize) -> i32 {
        let (ax, az) = self.coords(a);
        let (bx, bz) = self.coords(b);
        let dx = (ax - bx).abs();
        let dz = (az - bz).abs();
        let diag = dx.min(dz);
        let straight = dx.max(dz) - diag;
        diag * 141 + straight * 100
    }
}

const NEIGHBORS: [(i32, i32); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (-1, 1),
    (1, -1),
    (1, 1),
];

fn tri_cell_span(
    tri: [Vec3; 3],
    ox: f32,
    oz: f32,
    cell: f32,
    n: i32,
) -> Option<(i32, i32, i32, i32)> {
    let min_x = tri[0].x.min(tri[1].x).min(tri[2].x);
    let max_x = tri[0].x.max(tri[1].x).max(tri[2].x);
    let min_z = tri[0].z.min(tri[1].z).min(tri[2].z);
    let max_z = tri[0].z.max(tri[1].z).max(tri[2].z);
    let x0 = ((min_x - ox) / cell).floor() as i32;
    let x1 = ((max_x - ox) / cell).floor() as i32;
    let z0 = ((min_z - oz) / cell).floor() as i32;
    let z1 = ((max_z - oz) / cell).floor() as i32;
    if x1 < 0 || z1 < 0 || x0 >= n || z0 >= n {
        return None;
    }
    Some((
        x0.clamp(0, n - 1),
        z0.clamp(0, n - 1),
        x1.clamp(0, n - 1),
        z1.clamp(0, n - 1),
    ))
}

fn wall_column(tri: [Vec3; 3]) -> bool {
    let nrm = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
    let len = nrm.length();
    len > 1e-4 && nrm.y.abs() < len * 0.45
}

fn column_overlap(tri: [Vec3; 3], floor: f32) -> bool {
    let min_y = tri[0].y.min(tri[1].y).min(tri[2].y);
    let max_y = tri[0].y.max(tri[1].y).max(tri[2].y);
    max_y > floor + 12.0 && min_y < floor + 78.0
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

fn tri_overlaps_rect(tri: [Vec3; 3], min: bevy::prelude::Vec2, max: bevy::prelude::Vec2) -> bool {
    let p = [
        bevy::prelude::Vec2::new(tri[0].x, tri[0].z),
        bevy::prelude::Vec2::new(tri[1].x, tri[1].z),
        bevy::prelude::Vec2::new(tri[2].x, tri[2].z),
    ];
    let tmin_x = p[0].x.min(p[1].x).min(p[2].x);
    let tmax_x = p[0].x.max(p[1].x).max(p[2].x);
    let tmin_z = p[0].y.min(p[1].y).min(p[2].y);
    let tmax_z = p[0].y.max(p[1].y).max(p[2].y);
    if tmax_x < min.x || tmin_x > max.x || tmax_z < min.y || tmin_z > max.y {
        return false;
    }
    if p.iter()
        .any(|v| v.x >= min.x && v.x <= max.x && v.y >= min.y && v.y <= max.y)
    {
        return true;
    }
    let corners = [
        min,
        bevy::prelude::Vec2::new(max.x, min.y),
        max,
        bevy::prelude::Vec2::new(min.x, max.y),
    ];
    if corners.iter().any(|c| point_in_tri(*c, p)) {
        return true;
    }
    let edges = [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])];
    let rect = [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
    ];
    edges
        .into_iter()
        .any(|(a, b)| rect.into_iter().any(|(c, d)| seg_hit(a, b, c, d)))
}

fn point_in_tri(p: bevy::prelude::Vec2, t: [bevy::prelude::Vec2; 3]) -> bool {
    let d1 = edge(p, t[0], t[1]);
    let d2 = edge(p, t[1], t[2]);
    let d3 = edge(p, t[2], t[0]);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

fn edge(p: bevy::prelude::Vec2, a: bevy::prelude::Vec2, b: bevy::prelude::Vec2) -> f32 {
    (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y)
}

fn seg_hit(
    a: bevy::prelude::Vec2,
    b: bevy::prelude::Vec2,
    c: bevy::prelude::Vec2,
    d: bevy::prelude::Vec2,
) -> bool {
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    o1 * o2 < 0.0 && o3 * o4 < 0.0
}

fn orient(a: bevy::prelude::Vec2, b: bevy::prelude::Vec2, c: bevy::prelude::Vec2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_goes_around_a_wall() {
        let floor = [
            Vec3::new(-300.0, 0.0, -300.0),
            Vec3::new(-300.0, 0.0, 300.0),
            Vec3::new(300.0, 0.0, 300.0),
            Vec3::new(-300.0, 0.0, -300.0),
            Vec3::new(300.0, 0.0, 300.0),
            Vec3::new(300.0, 0.0, -300.0),
        ];
        let wall = [
            Vec3::new(0.0, 0.0, -30.0),
            Vec3::new(0.0, 0.0, 30.0),
            Vec3::new(0.0, 90.0, 30.0),
            Vec3::new(0.0, 0.0, -30.0),
            Vec3::new(0.0, 90.0, 30.0),
            Vec3::new(0.0, 90.0, -30.0),
        ];
        let mut tris = Vec::new();
        for chunk in floor.chunks_exact(3) {
            tris.push([chunk[0], chunk[1], chunk[2]]);
        }
        for chunk in wall.chunks_exact(3) {
            tris.push([chunk[0], chunk[1], chunk[2]]);
        }
        let col = Collider::from_tris(tris);
        let nav = NavMesh::bake(&col, Vec3::new(0.0, 20.0, 0.0), 256.0, 64.0);
        assert!(nav.walkable_count() > 8);
        let path = nav.find(Vec3::new(-140.0, 10.0, 0.0), Vec3::new(140.0, 10.0, 0.0));
        assert!(path.len() > 2, "path {path:?}");
        assert!(
            path.iter().any(|p| p.z.abs() > 40.0),
            "expected a detour, got {path:?}"
        );
    }
}
