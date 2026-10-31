//! LithTech 1.0 physics blocks.
//!
//! `FUN_00418d80` stores the on-disk PBlock table on the world: three grid
//! counts, a block-size vec3, an origin vec3, then one 8-byte block per cell.
//! Each block is a ushort node count, a ushort root, and `count * 6` bytes.
//! A node is three ushorts: BSP node index, negative-side child, positive-side
//! child. `0xFFFE` and `0xFFFF` end a side.
//!
//! `FUN_004129b0` is the query. It truncates `(pos - origin) / block_size`
//! toward zero, clamps into the grid, and walks that block. `FUN_00413270`
//! follows the positive child when the query is entirely in front of the BSP
//! node's plane and the negative child when it is entirely behind. A volume
//! that straddles the plane tests the node's polygon.
//!
//! The player is still a sphere with the tuned radius. The walk decides which
//! polygons can block, and the push is along the plane normal toward the
//! positive side, which is the direction `FUN_00414070` treats as clear. The
//! velocity solver, AABB clip (`FUN_004111f0`), and the 40-step impulse loop
//! are not reproduced here.

use bevy::prelude::Vec3;

use crate::collide::closest_on_tri;

const NODE_END: u16 = 0xfffe;

struct Plane {
    normal: Vec3,
    dist: f32,
}

struct Split {
    plane: u32,
    /// Index into [`PBlocks::polys`], or `u32::MAX` when the face is absent.
    poly: u32,
}

struct OctNode {
    bsp: u16,
    neg: u16,
    pos: u16,
}

struct Block {
    root: u16,
    start: u32,
    len: u32,
}

pub struct PBlocks {
    dims: [u32; 3],
    block_size: Vec3,
    origin: Vec3,
    blocks: Vec<Block>,
    nodes: Vec<OctNode>,
    splits: Vec<Split>,
    planes: Vec<Plane>,
    verts: Vec<Vec3>,
    /// `(first vertex, vertex count)` into `verts`.
    polys: Vec<(u32, u32)>,
}

impl PBlocks {
    pub fn dims(&self) -> [u32; 3] {
        self.dims
    }

    pub fn block_size(&self) -> Vec3 {
        self.block_size
    }

    /// Build the grid from a DAT v56 world. `None` when the file has no table.
    pub fn from_dat(data: &[u8]) -> Option<Self> {
        read_pblocks(data)
    }

    /// Push `center` out of any polygon the PBlock walk says the sphere
    /// straddles. `grounded` is a resolved contact whose plane faces up.
    pub fn depenetrate(&self, mut center: Vec3, radius: f32) -> (Vec3, bool) {
        let mut grounded = false;
        if !center.is_finite() || !(radius > 0.0) {
            return (center, false);
        }
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

    /// Highest upward face under `(x, z)` at or below `max_y`.
    pub fn floor_y(&self, x: f32, z: f32, max_y: f32) -> Option<f32> {
        if !x.is_finite() || !z.is_finite() || !max_y.is_finite() {
            return None;
        }
        self.floor_column(x, z, max_y)
    }

    fn floor_column(&self, x: f32, z: f32, max_y: f32) -> Option<f32> {
        let (ix, _, iz) = self.cell(Vec3::new(x, max_y, z));
        let mut best: Option<f32> = None;
        for cx in ix - 1..=ix + 1 {
            for cz in iz - 1..=iz + 1 {
                if cx < 0 || cz < 0 || cx >= self.dims[0] as i32 || cz >= self.dims[2] as i32 {
                    continue;
                }
                for cy in 0..self.dims[1] as i32 {
                    let y0 = self.origin.y + cy as f32 * self.block_size.y;
                    if y0 > max_y + 1.0 {
                        continue;
                    }
                    let top = (y0 + self.block_size.y).min(max_y);
                    if let Some(y) = self.column_floor(cx, cy, cz, x, z, y0, top, max_y) {
                        best = Some(best.map_or(y, |b| b.max(y)));
                    }
                }
            }
        }
        best
    }

    fn column_floor(
        &self,
        ix: i32,
        iy: i32,
        iz: i32,
        x: f32,
        z: f32,
        y0: f32,
        top: f32,
        max_y: f32,
    ) -> Option<f32> {
        let block = self.block_at(ix, iy, iz)?;
        if top < y0 {
            return None;
        }
        let mut best: Option<f32> = None;
        let mut stack = Vec::with_capacity(32);
        stack.push(block.root);
        let mut guard = 0u32;
        while let Some(idx) = stack.pop() {
            guard += 1;
            if guard > 20_000 {
                break;
            }
            if idx >= NODE_END {
                continue;
            }
            let Some(node) = self.node(block, idx) else {
                continue;
            };
            let Some(split) = self.splits.get(node.bsp as usize) else {
                continue;
            };
            let Some(plane) = self.planes.get(split.plane as usize) else {
                continue;
            };
            let d_top = plane.normal.dot(Vec3::new(x, top, z)) - plane.dist;
            let d_bot = plane.normal.dot(Vec3::new(x, y0, z)) - plane.dist;
            let crosses = d_top == 0.0 || d_bot == 0.0 || d_top.signum() != d_bot.signum();
            if crosses {
                if plane.normal.y > 0.5 {
                    if let Some(y) = self.poly_y(split.poly, x, z) {
                        if y <= max_y + 0.05 {
                            best = Some(best.map_or(y, |b| b.max(y)));
                        }
                    }
                }
            }
            if d_top > 0.0 && d_bot > 0.0 {
                stack.push(node.pos);
            } else if d_top < 0.0 && d_bot < 0.0 {
                stack.push(node.neg);
            } else {
                stack.push(node.neg);
                stack.push(node.pos);
            }
        }
        best
    }

    fn deepest(&self, center: Vec3, radius: f32) -> Option<(Vec3, f32)> {
        let (x0, y0, z0) = self.cell(center - Vec3::splat(radius));
        let (x1, y1, z1) = self.cell(center + Vec3::splat(radius));
        let mut best: Option<(Vec3, f32)> = None;
        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let Some(block) = self.block_at(x, y, z) else {
                        continue;
                    };
                    self.walk_sphere(block, center, radius, &mut best);
                }
            }
        }
        best
    }

    fn walk_sphere(&self, block: &Block, center: Vec3, radius: f32, best: &mut Option<(Vec3, f32)>) {
        let mut stack = Vec::with_capacity(32);
        stack.push(block.root);
        let mut guard = 0u32;
        while let Some(idx) = stack.pop() {
            guard += 1;
            if guard > 20_000 {
                break;
            }
            if idx >= NODE_END {
                continue;
            }
            let Some(node) = self.node(block, idx) else {
                continue;
            };
            let Some(split) = self.splits.get(node.bsp as usize) else {
                continue;
            };
            let Some(plane) = self.planes.get(split.plane as usize) else {
                continue;
            };
            let signed = plane.normal.dot(center) - plane.dist;
            if signed > radius {
                stack.push(node.pos);
                continue;
            }
            if signed < -radius {
                stack.push(node.neg);
                continue;
            }
            if let Some(depth) = self.poly_depth(split.poly, plane, center, radius, signed) {
                if best.map(|(_, d)| depth > d).unwrap_or(true) {
                    *best = Some((plane.normal, depth));
                }
            }
            stack.push(node.neg);
            stack.push(node.pos);
        }
    }

    fn poly_depth(
        &self,
        poly: u32,
        plane: &Plane,
        center: Vec3,
        radius: f32,
        signed: f32,
    ) -> Option<f32> {
        let (start, count) = *self.polys.get(poly as usize)?;
        let verts = self.poly_verts(start, count)?;
        if verts.len() < 3 {
            return None;
        }
        let mut nearest = f32::MAX;
        for i in 1..verts.len() - 1 {
            let q = closest_on_tri(center, verts[0], verts[i], verts[i + 1]);
            let dist = (center - q).length();
            if dist < nearest {
                nearest = dist;
            }
        }
        if nearest < radius {
            let depth = radius - signed;
            (depth > 0.0 && depth.is_finite() && plane.normal.is_finite()).then_some(depth)
        } else {
            None
        }
    }

    fn poly_y(&self, poly: u32, x: f32, z: f32) -> Option<f32> {
        let (start, count) = *self.polys.get(poly as usize)?;
        let verts = self.poly_verts(start, count)?;
        if verts.len() < 3 {
            return None;
        }
        for i in 1..verts.len() - 1 {
            if let Some(y) = bary_y([verts[0], verts[i], verts[i + 1]], x, z) {
                return Some(y);
            }
        }
        None
    }

    fn poly_verts(&self, start: u32, count: u32) -> Option<&[Vec3]> {
        let start = start as usize;
        let end = start + count as usize;
        self.verts.get(start..end)
    }

    fn node(&self, block: &Block, idx: u16) -> Option<&OctNode> {
        if idx as u32 >= block.len {
            return None;
        }
        self.nodes.get(block.start as usize + idx as usize)
    }

    fn block_at(&self, x: i32, y: i32, z: i32) -> Option<&Block> {
        if x < 0 || y < 0 || z < 0 {
            return None;
        }
        let (x, y, z) = (x as u32, y as u32, z as u32);
        if x >= self.dims[0] || y >= self.dims[1] || z >= self.dims[2] {
            return None;
        }
        let stride = self.dims[0].checked_mul(self.dims[1])?;
        let index = (x as usize)
            .checked_add((y as usize).checked_mul(self.dims[0] as usize)?)?
            .checked_add((z as usize).checked_mul(stride as usize)?)?;
        self.blocks.get(index)
    }

    /// `__ftol((pos - origin) / block_size)`, then clamp into the grid.
    fn cell(&self, p: Vec3) -> (i32, i32, i32) {
        (
            self.axis(p.x, 0),
            self.axis(p.y, 1),
            self.axis(p.z, 2),
        )
    }

    fn axis(&self, v: f32, axis: usize) -> i32 {
        let dim = self.dims[axis];
        if dim == 0 || !v.is_finite() {
            return 0;
        }
        let size = match axis {
            0 => self.block_size.x,
            1 => self.block_size.y,
            _ => self.block_size.z,
        };
        let origin = match axis {
            0 => self.origin.x,
            1 => self.origin.y,
            _ => self.origin.z,
        };
        if !(size > 0.0) {
            return 0;
        }
        let t = ((v - origin) / size) as i32;
        t.clamp(0, dim as i32 - 1)
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

fn sky_portal(name: &str) -> bool {
    name.rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("sky.dtx")
}

struct R<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.d.len() - self.p {
            return None;
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> Option<i16> {
        let b = self.take(2)?;
        Some(i16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Option<i32> {
        let b = self.take(4)?;
        Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Option<f32> {
        let b = self.take(4)?;
        Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn vec3(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }
    fn cstr(&mut self) -> Option<String> {
        let mut out = Vec::new();
        for _ in 0..200 {
            let c = self.u8()?;
            if c == 0 {
                return Some(String::from_utf8_lossy(&out).into_owned());
            }
            out.push(c);
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }
    fn ltstr(&mut self) -> Option<()> {
        let n = self.i16()?;
        if n < 0 || n > 4096 {
            return None;
        }
        self.skip(n as usize)
    }
}

fn read_pblocks(data: &[u8]) -> Option<PBlocks> {
    if data.len() < 16 || u32_at(data, 0) != 56 {
        return None;
    }
    let geo = u32_at(data, 8) as usize;
    if geo + 40 > data.len() {
        return None;
    }
    let mut r = R { d: data, p: geo };
    r.u32()?;
    for _ in 0..8 {
        r.u32()?;
    }
    r.skip(2)?;
    let mut ints = [0u32; 12];
    for slot in &mut ints {
        *slot = r.u32()?;
    }
    r.u32()?;
    r.vec3()?;
    r.vec3()?;
    r.vec3()?;
    let table_bytes = r.u32()? as usize;
    let ntex = r.u32()? as usize;
    if ntex == 0 || ntex >= 4000 || table_bytes >= 200_000 {
        return None;
    }
    let table_start = r.p;
    let mut textures = Vec::with_capacity(ntex);
    for _ in 0..ntex {
        textures.push(r.cstr()?);
    }
    if r.p - table_start != table_bytes {
        return None;
    }

    let point_count = ints[2] as usize;
    let plane_count = ints[3] as usize;
    let surface_count = ints[4] as usize;
    let portal_count = ints[5] as usize;
    let poly_count = ints[6] as usize;
    let leaf_count = ints[7];
    let node_count = ints[11] as usize;
    if point_count > 200_000
        || plane_count > 200_000
        || surface_count > 200_000
        || portal_count > 50_000
        || poly_count > 200_000
        || leaf_count > 200_000
        || node_count > 200_000
    {
        return None;
    }

    let mut vert_info = Vec::with_capacity(poly_count);
    for _ in 0..poly_count {
        vert_info.push((r.u8()?, r.u8()?));
    }
    skip_leaves(&mut r, leaf_count)?;

    let mut planes = Vec::with_capacity(plane_count);
    for _ in 0..plane_count {
        let n = r.vec3()?;
        planes.push(Plane {
            normal: Vec3::new(n[0], n[1], n[2]),
            dist: r.f32()?,
        });
    }

    let mut sky = Vec::with_capacity(surface_count);
    for _ in 0..surface_count {
        r.skip(72)?;
        let tex = r.u16()? as usize;
        r.skip(12)?;
        if r.u8()? != 0 {
            r.ltstr()?;
            r.ltstr()?;
        }
        r.u16()?;
        let name = textures.get(tex).map(String::as_str).unwrap_or("");
        sky.push(sky_portal(name));
    }

    let mut raw_polys = Vec::with_capacity(poly_count);
    for (c, e) in vert_info {
        r.skip(12)?;
        let surface = r.i32()?;
        let nv = c as usize + e as usize;
        if nv > 64 {
            return None;
        }
        let mut indices = Vec::with_capacity(nv);
        for _ in 0..nv {
            indices.push(r.i16()?);
            r.skip(3)?;
        }
        let solid = surface >= 0 && sky.get(surface as usize).copied() == Some(false);
        raw_polys.push((solid, indices));
    }

    let mut splits = Vec::with_capacity(node_count);
    let mut node_polys = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let plane = r.i32()?;
        let poly = r.i32()?;
        r.skip(26)?;
        node_polys.push((plane, poly));
    }
    for _ in 0..portal_count {
        r.ltstr()?;
        r.i32()?;
        r.u16()?;
        r.vec3()?;
        r.vec3()?;
    }
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        points.push(r.vec3()?);
    }

    let mut verts = Vec::new();
    let mut polys = Vec::new();
    let mut packed = vec![u32::MAX; raw_polys.len()];
    for (i, (solid, indices)) in raw_polys.iter().enumerate() {
        if !solid || indices.len() < 3 {
            continue;
        }
        let start = verts.len() as u32;
        let mut ok = true;
        let mark = verts.len();
        for &vi in indices {
            if vi < 0 {
                ok = false;
                break;
            }
            let Some(p) = points.get(vi as usize) else {
                ok = false;
                break;
            };
            verts.push(Vec3::new(p[0], p[1], p[2]));
        }
        if !ok {
            verts.truncate(mark);
            continue;
        }
        packed[i] = polys.len() as u32;
        polys.push((start, indices.len() as u32));
    }
    for (plane, poly) in node_polys {
        let poly = if poly >= 0 {
            packed.get(poly as usize).copied().unwrap_or(u32::MAX)
        } else {
            u32::MAX
        };
        let plane = if plane >= 0 { plane as u32 } else { u32::MAX };
        splits.push(Split { plane, poly });
    }

    let dx = r.u32()?;
    let dy = r.u32()?;
    let dz = r.u32()?;
    let nblocks = (dx as usize)
        .checked_mul(dy as usize)?
        .checked_mul(dz as usize)?;
    if nblocks == 0 || nblocks > 200_000 || dx == 0 || dy == 0 || dz == 0 {
        return None;
    }
    let size = r.vec3()?;
    let origin = r.vec3()?;
    if size[0] <= 0.0 || size[1] <= 0.0 || size[2] <= 0.0 {
        return None;
    }

    let mut blocks = Vec::with_capacity(nblocks);
    let mut nodes = Vec::new();
    for _ in 0..nblocks {
        let count = r.u16()? as usize;
        let root = r.u16()?;
        if count > 4000 {
            return None;
        }
        let start = nodes.len() as u32;
        let bytes = r.take(count * 6)?;
        for k in 0..count {
            let o = k * 6;
            let bsp = u16::from_le_bytes([bytes[o], bytes[o + 1]]);
            let neg = u16::from_le_bytes([bytes[o + 2], bytes[o + 3]]);
            let pos = u16::from_le_bytes([bytes[o + 4], bytes[o + 5]]);
            nodes.push(OctNode { bsp, neg, pos });
        }
        blocks.push(Block {
            root,
            start,
            len: count as u32,
        });
    }

    Some(PBlocks {
        dims: [dx, dy, dz],
        block_size: Vec3::new(size[0], size[1], size[2]),
        origin: Vec3::new(origin[0], origin[1], origin[2]),
        blocks,
        nodes,
        splits,
        planes,
        verts,
        polys,
    })
}

fn skip_leaves(r: &mut R, n: u32) -> Option<()> {
    for _ in 0..n {
        let llc = r.u16()?;
        if llc == 0xffff {
            r.u16()?;
        } else {
            if llc > 4000 {
                return None;
            }
            for _ in 0..llc {
                r.u16()?;
                let sz = r.u16()? as usize;
                if sz > 20_000 {
                    return None;
                }
                r.skip(sz)?;
            }
        }
        let pc = r.u16()? as usize;
        if pc > 8000 {
            return None;
        }
        r.skip(pc * 4)?;
        r.f32()?;
    }
    Some(())
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_quad() -> PBlocks {
        PBlocks {
            dims: [1, 1, 1],
            block_size: Vec3::splat(500.0),
            origin: Vec3::ZERO,
            blocks: vec![Block {
                root: 0,
                start: 0,
                len: 1,
            }],
            nodes: vec![OctNode {
                bsp: 0,
                neg: 0xffff,
                pos: 0xfffe,
            }],
            splits: vec![Split { plane: 0, poly: 0 }],
            planes: vec![Plane {
                normal: Vec3::Y,
                dist: 0.0,
            }],
            verts: vec![
                Vec3::new(-10.0, 0.0, -10.0),
                Vec3::new(10.0, 0.0, -10.0),
                Vec3::new(10.0, 0.0, 10.0),
                Vec3::new(-10.0, 0.0, 10.0),
            ],
            polys: vec![(0, 4)],
        }
    }

    fn wall() -> PBlocks {
        PBlocks {
            dims: [1, 1, 1],
            block_size: Vec3::splat(500.0),
            origin: Vec3::ZERO,
            blocks: vec![Block {
                root: 0,
                start: 0,
                len: 1,
            }],
            nodes: vec![OctNode {
                bsp: 0,
                neg: 0xffff,
                pos: 0xfffe,
            }],
            splits: vec![Split { plane: 0, poly: 0 }],
            planes: vec![Plane {
                normal: Vec3::X,
                dist: 0.0,
            }],
            verts: vec![
                Vec3::new(0.0, 0.0, -10.0),
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(0.0, 20.0, 10.0),
                Vec3::new(0.0, 20.0, -10.0),
            ],
            polys: vec![(0, 4)],
        }
    }

    #[test]
    fn sphere_rests_on_the_floor_and_misses_open_air() {
        let world = floor_quad();
        let (hit, grounded) = world.depenetrate(Vec3::new(0.0, 10.0, 0.0), 16.0);
        assert!(grounded);
        assert!((hit.y - 16.05).abs() < 0.02, "y {}", hit.y);
        assert!(hit.x.abs() < 1e-3 && hit.z.abs() < 1e-3);

        let (miss, air) = world.depenetrate(Vec3::new(0.0, 40.0, 0.0), 16.0);
        assert!(!air);
        assert!((miss.y - 40.0).abs() < 1e-3);

        // Straddles the infinite plane, outside the quad.
        let (aside, aside_ground) = world.depenetrate(Vec3::new(40.0, 10.0, 0.0), 16.0);
        assert!(!aside_ground);
        assert!((aside.y - 10.0).abs() < 1e-3);

        assert!((world.floor_y(0.0, 0.0, 5.0).unwrap() - 0.0).abs() < 1e-3);
        assert!(world.floor_y(80.0, 80.0, 5.0).is_none());
        assert!(world.floor_y(0.0, 0.0, -1.0).is_none());
    }

    #[test]
    fn wall_push_slides_along_the_plane() {
        let world = wall();
        let (pushed, grounded) = world.depenetrate(Vec3::new(4.0, 10.0, 0.0), 8.0);
        assert!(!grounded);
        assert!((pushed.x - 8.05).abs() < 0.02, "x {}", pushed.x);
        assert!((pushed.y - 10.0).abs() < 1e-3);
        assert!(pushed.z.abs() < 1e-3);
    }

    #[test]
    fn grid_index_matches_ambush_block_size() {
        let world = PBlocks {
            dims: [25, 12, 30],
            block_size: Vec3::splat(500.0),
            origin: Vec3::new(-6144.0, -640.0, -6144.0),
            blocks: Vec::new(),
            nodes: Vec::new(),
            splits: Vec::new(),
            planes: Vec::new(),
            verts: Vec::new(),
            polys: Vec::new(),
        };
        assert_eq!(world.cell(Vec3::new(3072.0, 32.0, 2688.0)), (18, 1, 17));
        assert_eq!(world.cell(Vec3::new(-10_000.0, 0.0, 0.0)).0, 0);
    }

    #[test]
    fn ambush_dat_has_a_floor_under_start() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
        let Ok(data) = std::fs::read(&path) else {
            return;
        };
        let world = PBlocks::from_dat(&data).expect("ambush pblocks");
        assert_eq!(world.dims(), [25, 12, 30]);
        assert!((world.block_size().x - 500.0).abs() < 1e-3);
        assert!((world.origin.x - -6144.0).abs() < 1e-3);
        let fy = world
            .floor_y(3072.0, 2688.0, 160.0)
            .expect("floor under start1");
        assert!((-64.0..128.0).contains(&fy), "floor {fy}");
        let (center, grounded) = world.depenetrate(Vec3::new(3072.0, fy + 10.0, 2688.0), 16.0);
        assert!(grounded);
        assert!((center.y - (fy + 16.05)).abs() < 1.0, "center {}", center.y);
    }
}
