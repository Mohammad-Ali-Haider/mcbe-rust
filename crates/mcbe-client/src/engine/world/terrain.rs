//! Versioned deterministic climate, biome palettes and 3D density sampling.
use crate::{
    engine,
    engine_blocks::{BlockId, Registry},
    scene::Result,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Biome {
    pub id: String,
    pub label: String,
    pub surface: String,
    pub subsurface: String,
    pub trees: u8,
    pub temperature: [f64; 2],
    pub moisture: [f64; 2],
    pub altitude: [i32; 2],
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub sea_level: i32,
    pub caves: bool,
    pub ravines: bool,
    pub biomes: Vec<Biome>,
}
impl Settings {
    pub fn built_in() -> Self {
        serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/world/terrain.json"
        )))
        .expect("Terrain settings")
    }
    pub fn read_home(home: &std::path::Path) -> Result<Self> {
        let p = home.join("content/base/world/terrain.json");
        if !p.is_file() {
            return Ok(Self::built_in());
        }
        if std::fs::metadata(&p)?.len() > 256 * 1024 {
            return Err("Terrain JSON exceeds 256 KiB".into());
        }
        Ok(serde_json::from_slice(&std::fs::read(p)?)?)
    }
}
#[derive(Clone)]
struct Palette {
    top: BlockId,
    soil: BlockId,
}
#[derive(Clone)]
pub struct Terrain {
    pub settings: Settings,
    seed: u64,
    palettes: Vec<Palette>,
    stone: BlockId,
    deep: BlockId,
    pub water: BlockId,
    pub leaves: BlockId,
    pub log: BlockId,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Sample {
    pub height: i32,
    pub temperature: f64,
    pub moisture: f64,
    pub biome: usize,
    pub water: Option<i32>,
    pub ravine: bool,
}
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6. - 15.) + 10.)
}
impl Terrain {
    pub fn new(seed: u64, r: &Registry, settings: Settings) -> Result<Self> {
        if settings.version != 1
            || !(-1024..=1024).contains(&settings.sea_level)
            || settings.biomes.len() > 64
            || settings.biomes.is_empty()
        {
            return Err("Invalid terrain settings".into());
        }
        let mut names = std::collections::BTreeSet::new();
        let mut palettes = Vec::new();
        for b in &settings.biomes {
            if !engine::blocks::state::identifier(&b.id)
                || !names.insert(b.id.clone())
                || b.label.len() > 64
                || b.trees > 100
                || b.temperature
                    .iter()
                    .chain(&b.moisture)
                    .any(|n| !n.is_finite() || !(-1. ..=1.).contains(n))
                || b.temperature[0] > b.temperature[1]
                || b.moisture[0] > b.moisture[1]
                || b.altitude[0] > b.altitude[1]
            {
                return Err("Invalid biome definition".into());
            }
            palettes.push(Palette {
                top: r.find(&b.surface)?,
                soil: r.find(&b.subsurface)?,
            });
        }
        for name in ["ocean", "river", "plains"] {
            if !names.contains(name) {
                return Err("Terrain requires ocean, river and plains biomes".into());
            }
        }
        Ok(Self {
            settings,
            seed,
            palettes,
            stone: r.find("slate")?,
            deep: r.find("basalt")?,
            water: r.find("water")?,
            leaves: r.find("oak_leaves")?,
            log: r.find("oak_log")?,
        })
    }
    fn hash(&self, x: i32, y: i32, z: i32, salt: u64) -> f64 {
        let n = engine::noise(
            x,
            z,
            self.seed ^ salt ^ (y as u64).wrapping_mul(0x9e3779b97f4a7c15),
        );
        (n >> 11) as f64 / (1u64 << 53) as f64 * 2. - 1.
    }
    fn n2(&self, x: i32, z: i32, scale: i32, salt: u64) -> f64 {
        let gx = x.div_euclid(scale);
        let gz = z.div_euclid(scale);
        let u = fade(x.rem_euclid(scale) as f64 / scale as f64);
        let v = fade(z.rem_euclid(scale) as f64 / scale as f64);
        let a = self.hash(gx, 0, gz, salt) * (1. - u) + self.hash(gx + 1, 0, gz, salt) * u;
        let b = self.hash(gx, 0, gz + 1, salt) * (1. - u) + self.hash(gx + 1, 0, gz + 1, salt) * u;
        a * (1. - v) + b * v
    }
    fn n3(&self, p: [i32; 3], scale: i32, salt: u64) -> f64 {
        let g = p.map(|v| v.div_euclid(scale));
        let t = p.map(|v| fade(v.rem_euclid(scale) as f64 / scale as f64));
        let mut n = 0.;
        for x in 0..2 {
            for y in 0..2 {
                for z in 0..2 {
                    n += self.hash(g[0] + x, g[1] + y, g[2] + z, salt)
                        * if x == 0 { 1. - t[0] } else { t[0] }
                        * if y == 0 { 1. - t[1] } else { t[1] }
                        * if z == 0 { 1. - t[2] } else { t[2] };
                }
            }
        }
        n
    }
    fn named(&self, id: &str) -> usize {
        self.settings
            .biomes
            .iter()
            .position(|b| b.id == id)
            .expect("Required biome")
    }
    pub fn sample(&self, x: i32, z: i32) -> Sample {
        let sea = self.settings.sea_level;
        let continent = self.n2(x, z, 768, 0x1001);
        let temperature = self.n2(x, z, 1024, 0x1002);
        let moisture = self.n2(x, z, 640, 0x1003);
        let hills = self.n2(x, z, 96, 0x1004) * 12. + self.n2(x, z, 32, 0x1005) * 3.;
        let ridges = (1. - self.n2(x, z, 256, 0x1006).abs()).powi(3);
        let mountains = ((continent - 0.05) * 2.).clamp(0., 1.) * ridges * 100.;
        let mut height = sea as f64 + 12. + continent * 50. + hills + mountains;
        let ocean = continent < -0.22;
        let river = self.n2(x, z, 512, 0x1007).abs();
        let river_level = sea + (continent.max(0.) * 4.) as i32;
        if !ocean && river < 0.065 && height > river_level as f64 - 3. {
            let t = (river / 0.065).clamp(0., 1.);
            let t = fade(t);
            height = (river_level - 4) as f64 * (1. - t) + height * t;
        }
        let mut ravine = false;
        if self.settings.ravines && !ocean && river > 0.09 {
            let cx = x.div_euclid(512);
            let cz = z.div_euclid(512);
            let n = engine::noise(cx, cz, self.seed ^ 0x2001);
            if n.is_multiple_of(3) && (48..464).contains(&x.rem_euclid(512)) {
                let middle = cz * 512 + 256 + (self.n2(x, 0, 128, 0x2002) * 80.) as i32;
                let width = 4. + self.n2(x, z, 64, 0x2003).abs() * 3.;
                let distance = (z - middle).abs() as f64;
                if distance < width {
                    let drop = (1. - (distance / width).powi(3)) * 34.;
                    height -= drop;
                    ravine = true;
                }
            }
        }
        let height = height.floor() as i32;
        let water = if ocean && height < sea {
            Some(sea)
        } else if river < 0.025 && height < river_level {
            Some(river_level)
        } else {
            None
        };
        let biome = if ocean {
            self.named("ocean")
        } else if river < 0.025 {
            self.named("river")
        } else {
            self.settings
                .biomes
                .iter()
                .position(|b| {
                    !matches!(b.id.as_str(), "ocean" | "river")
                        && temperature >= b.temperature[0]
                        && temperature <= b.temperature[1]
                        && moisture >= b.moisture[0]
                        && moisture <= b.moisture[1]
                        && height >= b.altitude[0]
                        && height <= b.altitude[1]
                })
                .unwrap_or_else(|| self.named("plains"))
        };
        Sample {
            height,
            temperature,
            moisture,
            biome,
            water,
            ravine,
        }
    }
    pub fn tree(&self, c: [i32; 2]) -> Option<[i32; 3]> {
        let n = engine::noise(c[0], c[1], self.seed ^ 0x38c6f01c);
        let x = c[0] * 16 + 4 + ((n >> 8) % 8) as i32;
        let z = c[1] * 16 + 4 + ((n >> 16) % 8) as i32;
        let s = self.sample(x, z);
        if s.water.is_some()
            || s.ravine
            || n % 100 >= u64::from(self.settings.biomes[s.biome].trees)
        {
            None
        } else {
            Some([x, s.height, z])
        }
    }
    pub fn solid_surface(&self, s: Sample) -> (i32, BlockId) {
        (s.height, self.palettes[s.biome].top)
    }
    pub fn surface(&self, s: Sample) -> (i32, BlockId) {
        if let Some(w) = s.water {
            (w, self.water)
        } else {
            (s.height, self.palettes[s.biome].top)
        }
    }
    pub fn block(&self, p: [i32; 3], s: Sample, tree: Option<[i32; 3]>) -> Option<BlockId> {
        let [x, y, z] = p;
        if y >= s.height {
            if s.water.is_some_and(|w| y < w) {
                return Some(self.water);
            }
            if let Some([tx, ty, tz]) = tree {
                if x == tx && z == tz && (ty..ty + 4).contains(&y) {
                    return Some(self.log);
                }
                if (x - tx).abs() <= 2
                    && (z - tz).abs() <= 2
                    && (x - tx).abs() + (z - tz).abs() < 4
                    && (ty + 2..=ty + 4).contains(&y)
                {
                    return Some(self.leaves);
                }
            }
            return None;
        }
        if self.settings.caves && y < s.height - 7 {
            let worm = self.n3(p, 32, 0x3001).abs() < 0.12 && self.n3(p, 24, 0x3002).abs() < 0.14;
            let room = self.n3(p, 48, 0x3003) > 0.62;
            if worm || room {
                return None;
            }
        }
        Some(if y == s.height - 1 {
            self.palettes[s.biome].top
        } else if y >= s.height - 4 {
            self.palettes[s.biome].soil
        } else if y < -64 {
            self.deep
        } else {
            self.stone
        })
    }
    pub fn landmark(&self, index: usize) -> [i32; 3] {
        let desired = ["plains", "forest", "desert", "snow"][index % 4];
        for radius in 0..=32i32 {
            for x in -radius..=radius {
                for z in -radius..=radius {
                    if x.abs() != radius && z.abs() != radius {
                        continue;
                    }
                    let (x, z) = (x * 64, z * 64);
                    let sample = self.sample(x, z);
                    if self.settings.biomes[sample.biome].id == desired
                        && sample.water.is_none()
                        && !sample.ravine
                    {
                        return [x, sample.height + 6, z];
                    }
                }
            }
        }
        self.spawn()
    }
    pub fn spawn(&self) -> [i32; 3] {
        for radius in 0..=16i32 {
            for x in -radius..=radius {
                for z in -radius..=radius {
                    if x.abs() != radius && z.abs() != radius {
                        continue;
                    }
                    let (x, z) = (x * 32, z * 32);
                    let s = self.sample(x, z);
                    if s.water.is_none() && !s.ravine && s.height < 128 {
                        return [x, s.height + 6, z];
                    }
                }
            }
        }
        let s = self.sample(0, 0);
        [0, s.water.unwrap_or(s.height) + 6, 0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn climate_biomes_rivers_oceans_ravines_and_caves_are_deterministic() {
        let r = Registry::built_in();
        let t = Terrain::new(360036, &r, Settings::built_in()).unwrap();
        let mut biomes = std::collections::BTreeSet::new();
        let mut oceans = 0;
        let mut rivers = 0;
        let mut ravines = 0;
        for x in (-2048..=2048).step_by(32) {
            for z in (-2048..=2048).step_by(32) {
                let s = t.sample(x, z);
                biomes.insert(t.settings.biomes[s.biome].id.clone());
                assert_eq!(s.height, t.sample(x, z).height);
                if let Some(level) = s.water {
                    if t.settings.biomes[s.biome].id == "ocean" {
                        oceans += 1;
                    } else {
                        rivers += 1;
                    }
                    assert_eq!(t.block([x, level - 1, z], s, None), Some(t.water));
                }
                ravines += usize::from(s.ravine);
            }
        }
        for id in [
            "plains",
            "forest",
            "desert",
            "mountains",
            "snow",
            "river",
            "ocean",
        ] {
            assert!(biomes.contains(id), "missing {id}");
        }
        assert!(oceans > 10 && rivers > 10 && ravines > 0);
        let mut cave = 0;
        let mut rock = 0;
        for x in (-128..128).step_by(8) {
            for z in (-128..128).step_by(8) {
                let s = t.sample(x, z);
                for depth in [20, 40, 60, 80, 100] {
                    let id = t.block([x, s.height - depth, z], s, None);
                    if id.is_none() {
                        cave += 1;
                    } else {
                        rock += 1;
                    }
                }
                assert!(t.block([x, s.height - 1, z], s, None).is_some());
            }
        }
        assert!(cave > 10 && rock > 100);
        let different = Terrain::new(360037, &r, Settings::built_in()).unwrap();
        assert_ne!(
            t.sample(723, -391).height,
            different.sample(723, -391).height
        );
    }
    #[test]
    fn biome_registration_rejects_invalid_palettes_ranges_and_duplicates() {
        let r = Registry::built_in();
        let mut s = Settings::built_in();
        s.biomes.push(s.biomes[0].clone());
        assert!(Terrain::new(1, &r, s).is_err());
        let mut s = Settings::built_in();
        s.biomes[0].surface = "missing".into();
        assert!(Terrain::new(1, &r, s).is_err());
        let mut s = Settings::built_in();
        s.biomes[2].temperature = [0.5, -0.5];
        assert!(Terrain::new(1, &r, s).is_err());
    }
}
