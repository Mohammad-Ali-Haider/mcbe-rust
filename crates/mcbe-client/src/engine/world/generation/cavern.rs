//! An open cavern between separately sampled floor and ceiling intervals.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cavern {
    pub version: u32,
    pub floor: i32,
    pub floor_scale: i32,
    pub floor_height: f64,
    pub clearance: i32,
    pub ceiling_scale: i32,
    pub ceiling_height: f64,
    pub roof_thickness: i32,
    pub water_level: Option<i32>,
}
impl Default for Cavern {
    fn default() -> Self {
        Self {
            version: 1,
            floor: 24,
            floor_scale: 96,
            floor_height: 18.,
            clearance: 48,
            ceiling_scale: 128,
            ceiling_height: 10.,
            roof_thickness: 20,
            water_level: Some(16),
        }
    }
}
pub fn compile(value: &serde_json::Value) -> Result<Arc<dyn Generator>> {
    let s: Cavern = serde_json::from_value(value.clone())?;
    if s.version != 1
        || !(-4096..=4096).contains(&s.floor)
        || !scale(s.floor_scale)
        || !scale(s.ceiling_scale)
        || !amplitude(s.floor_height)
        || !amplitude(s.ceiling_height)
        || !(12..=256).contains(&s.clearance)
        || s.ceiling_height > s.clearance as f64 - 8.
        || !(4..=64).contains(&s.roof_thickness)
        || s.water_level.is_some_and(|v| !(-4096..=4096).contains(&v))
    {
        return Err("Invalid cavern generator options".into());
    }
    Ok(Arc::new(s))
}
impl Generator for Cavern {
    fn canonical_options(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(self).expect("Validated generator options"))
    }
    fn residency(&self) -> Option<[(i32, i32); 2]> {
        let d = self.floor_height.ceil() as i32;
        let c = self.ceiling_height.ceil() as i32;
        Some([
            (self.floor - d - 64, self.floor + d + 6),
            (
                self.floor - d + self.clearance - c,
                self.floor + d + self.clearance + c + self.roof_thickness,
            ),
        ])
    }
    fn column(&self, n: Noise, _sea: i32, x: i32, z: i32) -> Column {
        let top =
            self.floor + (n.at2(x, z, self.floor_scale, 0x4101) * self.floor_height).floor() as i32;
        let ceiling = top
            + self.clearance
            + (n.at2(x, z, self.ceiling_scale, 0x4102) * self.ceiling_height).floor() as i32;
        let mut out = Column::surface(top);
        out.ceiling = Some(ceiling);
        out.roof_top = Some(ceiling + self.roof_thickness);
        out.water = self.water_level.filter(|v| *v > top && *v < ceiling);
        out
    }
}
