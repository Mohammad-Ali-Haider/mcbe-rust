//! Climate surface with editable continental, hill, mountain and river scales.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Surface {
    pub version: u32,
    pub base_height: i32,
    pub continent_scale: i32,
    pub continent_height: f64,
    pub hill_scale: i32,
    pub hill_height: f64,
    pub detail_scale: i32,
    pub detail_height: f64,
    pub mountain_scale: i32,
    pub mountain_height: f64,
    pub ocean_threshold: f64,
    pub oceans: bool,
    pub rivers: bool,
    pub river_scale: i32,
    pub river_width: f64,
    pub river_depth: i32,
}
impl Default for Surface {
    fn default() -> Self {
        Self {
            version: 1,
            base_height: 12,
            continent_scale: 768,
            continent_height: 50.,
            hill_scale: 96,
            hill_height: 12.,
            detail_scale: 32,
            detail_height: 3.,
            mountain_scale: 256,
            mountain_height: 100.,
            ocean_threshold: -0.22,
            oceans: true,
            rivers: true,
            river_scale: 512,
            river_width: 0.065,
            river_depth: 4,
        }
    }
}
pub fn compile(value: &serde_json::Value) -> Result<Arc<dyn Generator>> {
    let s: Surface = serde_json::from_value(value.clone())?;
    if s.version != 1
        || ![
            s.continent_scale,
            s.hill_scale,
            s.detail_scale,
            s.mountain_scale,
            s.river_scale,
        ]
        .into_iter()
        .all(scale)
        || ![
            s.continent_height,
            s.hill_height,
            s.detail_height,
            s.mountain_height,
        ]
        .into_iter()
        .all(amplitude)
        || !(-4096..=4096).contains(&s.base_height)
        || !s.ocean_threshold.is_finite()
        || !(-1. ..=1.).contains(&s.ocean_threshold)
        || !s.river_width.is_finite()
        || !(0.001..=0.25).contains(&s.river_width)
        || !(1..=64).contains(&s.river_depth)
    {
        return Err("Invalid surface generator options".into());
    }
    Ok(Arc::new(s))
}
impl Generator for Surface {
    fn canonical_options(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(self).expect("Validated generator options"))
    }
    fn layered(&self) -> bool {
        false
    }
    fn ravines(&self) -> bool {
        true
    }
    fn column(&self, n: Noise, sea: i32, x: i32, z: i32) -> Column {
        let c = n.at2(x, z, self.continent_scale, 0x1001);
        let ridges = (1. - n.at2(x, z, self.mountain_scale, 0x1006).abs()).powi(3);
        let mut height = sea as f64
            + self.base_height as f64
            + c * self.continent_height
            + n.at2(x, z, self.hill_scale, 0x1004) * self.hill_height
            + n.at2(x, z, self.detail_scale, 0x1005) * self.detail_height
            + ((c - 0.05) * 2.).clamp(0., 1.) * ridges * self.mountain_height;
        let ocean = self.oceans && c < self.ocean_threshold;
        let river = n.at2(x, z, self.river_scale, 0x1007).abs();
        let level = sea + (c.max(0.) * 4.) as i32;
        if self.rivers
            && !ocean
            && river < self.river_width
            && height > (level - self.river_depth) as f64
        {
            let t = (river / self.river_width).clamp(0., 1.);
            let t = t * t * t * (t * (t * 6. - 15.) + 10.);
            height = (level - self.river_depth) as f64 * (1. - t) + height * t;
        }
        let mut out = Column::surface(height.floor() as i32);
        if ocean {
            out.wet = WaterBiome::Ocean;
            if out.top < sea {
                out.water = Some(sea);
            }
        } else if self.rivers && river < self.river_width * 0.385 {
            out.wet = WaterBiome::River;
            if out.top < level {
                out.water = Some(level);
            }
        }
        out
    }
}
