//! Deterministic elliptical archipelago with real voids and tapered undersides.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Islands {
    pub version: u32,
    pub spacing: i32,
    pub radius: i32,
    pub altitude: i32,
    pub altitude_variation: i32,
    pub thickness: i32,
    pub relief: f64,
    pub detail_scale: i32,
    pub detail_height: f64,
}
impl Default for Islands {
    fn default() -> Self {
        Self {
            version: 1,
            spacing: 192,
            radius: 72,
            altitude: 112,
            altitude_variation: 24,
            thickness: 32,
            relief: 14.,
            detail_scale: 32,
            detail_height: 3.,
        }
    }
}
pub fn compile(value: &serde_json::Value) -> Result<Arc<dyn Generator>> {
    let s: Islands = serde_json::from_value(value.clone())?;
    if s.version != 1
        || !(48..=512).contains(&s.spacing)
        || !(12..=192).contains(&s.radius)
        || s.radius * 2 > s.spacing
        || !(-4096..=4096).contains(&s.altitude)
        || !(0..=256).contains(&s.altitude_variation)
        || !(4..=128).contains(&s.thickness)
        || !amplitude(s.relief)
        || !scale(s.detail_scale)
        || !amplitude(s.detail_height)
    {
        return Err("Invalid island generator options".into());
    }
    Ok(Arc::new(s))
}
impl Generator for Islands {
    fn canonical_options(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(self).expect("Validated generator options"))
    }
    fn intersects(&self, n: Noise, rect: [i32; 4]) -> bool {
        let [xmin, zmin, xmax, zmax] = rect;
        for a in xmin.div_euclid(self.spacing) - 1..=xmax.div_euclid(self.spacing) + 1 {
            for b in zmin.div_euclid(self.spacing) - 1..=zmax.div_euclid(self.spacing) + 1 {
                let h = n.hash(a, b, 0x5101);
                let jitter = (self.spacing - self.radius * 2).max(0) / 3;
                let dx = if jitter == 0 {
                    0
                } else {
                    ((h >> 8) % (2 * jitter + 1) as u64) as i32 - jitter
                };
                let dz = if jitter == 0 {
                    0
                } else {
                    ((h >> 24) % (2 * jitter + 1) as u64) as i32 - jitter
                };
                let cx = a * self.spacing + self.spacing / 2 + dx;
                let cz = b * self.spacing + self.spacing / 2 + dz;
                let rx = self.radius as f64 * (0.78 + ((h >> 40) & 255) as f64 / 255. * 0.22);
                let rz = self.radius as f64 * (0.78 + ((h >> 48) & 255) as f64 / 255. * 0.22);
                let x = cx.clamp(xmin, xmax);
                let z = cz.clamp(zmin, zmax);
                if ((x - cx) as f64 / rx).powi(2) + ((z - cz) as f64 / rz).powi(2) < 1. {
                    return true;
                }
            }
        }
        false
    }

    fn residency(&self) -> Option<[(i32, i32); 2]> {
        Some([
            (
                self.altitude
                    - self.altitude_variation
                    - self.detail_height.ceil() as i32
                    - self.thickness
                    - 3,
                self.altitude
                    + self.altitude_variation
                    + self.relief.ceil() as i32
                    + self.detail_height.ceil() as i32
                    + 6,
            ),
            (0, 0),
        ])
    }
    fn column(&self, n: Noise, _sea: i32, x: i32, z: i32) -> Column {
        let gx = x.div_euclid(self.spacing);
        let gz = z.div_euclid(self.spacing);
        let mut best: Option<(f64, u64)> = None;
        for a in gx - 1..=gx + 1 {
            for b in gz - 1..=gz + 1 {
                let h = n.hash(a, b, 0x5101);
                let jitter = (self.spacing - self.radius * 2).max(0) / 3;
                let dx = if jitter == 0 {
                    0
                } else {
                    ((h >> 8) % (2 * jitter + 1) as u64) as i32 - jitter
                };
                let dz = if jitter == 0 {
                    0
                } else {
                    ((h >> 24) % (2 * jitter + 1) as u64) as i32 - jitter
                };
                let cx = a * self.spacing + self.spacing / 2 + dx;
                let cz = b * self.spacing + self.spacing / 2 + dz;
                let rx = self.radius as f64 * (0.78 + ((h >> 40) & 255) as f64 / 255. * 0.22);
                let rz = self.radius as f64 * (0.78 + ((h >> 48) & 255) as f64 / 255. * 0.22);
                let q = ((x - cx) as f64 / rx).powi(2) + ((z - cz) as f64 / rz).powi(2);
                if best.is_none_or(|(v, _)| q < v) {
                    best = Some((q, h));
                }
            }
        }
        let Some((q, h)) = best else {
            let mut c = Column::surface(self.altitude);
            c.present = false;
            return c;
        };
        let variation = if self.altitude_variation == 0 {
            0
        } else {
            ((h >> 32) % (self.altitude_variation * 2 + 1) as u64) as i32 - self.altitude_variation
        };
        let shape = (1. - q).max(0.).sqrt();
        let top = self.altitude
            + variation
            + (shape * self.relief + n.at2(x, z, self.detail_scale, 0x5102) * self.detail_height)
                .floor() as i32;
        let mut c = Column::surface(top);
        c.present = q < 1.;
        c.coverage = 1. - q;
        c.bottom = Some(top - 2 - (shape * self.thickness as f64).floor() as i32);
        c
    }
}
