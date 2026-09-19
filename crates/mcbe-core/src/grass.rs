//! Resource-derived grass tint prototype. Climate sampling and alpha masking
//! remain inferred until a matching native client frame can be measured.
use crate::{
    Result,
    resources::{self, Pack, ResourceError},
    textures::Texture,
};
use serde_json::{Value, json};

pub struct Tint {
    pub rgb: [u8; 3],
    pub provenance: Value,
}
fn identifier(id: &str) -> Result<&str> {
    let name = id
        .strip_prefix("minecraft:")
        .ok_or("Explicit minecraft biome namespace required")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("Unsupported biome identifier".into());
    }
    Ok(name)
}
fn number(v: &Value, name: &str) -> Result<f32> {
    let result = v[name]
        .as_f64()
        .ok_or("Explicit numeric climate field required")? as f32;
    if !result.is_finite() {
        return Err("Nonfinite climate field".into());
    }
    Ok(result)
}
pub fn coordinates(temperature: f32, downfall: f32) -> Result<[u8; 2]> {
    if !temperature.is_finite() || !downfall.is_finite() {
        return Err("Nonfinite climate input".into());
    }
    let temperature = temperature.clamp(0.0, 1.0);
    let moisture = downfall.clamp(0.0, 1.0) * temperature;
    Ok([
        ((1.0 - temperature) * 255.0) as u8,
        ((1.0 - moisture) * 255.0) as u8,
    ])
}
enum Source {
    Fixed([u8; 3]),
    Map(String),
}
fn source(value: &Value) -> Result<Source> {
    if let Some(hex) = value.as_str() {
        let bytes = hex
            .strip_prefix('#')
            .ok_or("Expected #RRGGBB grass color")?;
        if bytes.len() != 6 || !bytes.is_ascii() {
            return Err("Expected six ASCII grass color digits".into());
        }
        let color = (0..3)
            .map(|i| {
                u8::from_str_radix(&bytes[i * 2..i * 2 + 2], 16)
                    .map_err(|_| "Invalid grass color digit".to_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(Source::Fixed(color.try_into().unwrap()));
    }
    if let Some(object) = value.as_object()
        && object.len() == 1
    {
        let map = object
            .get("color_map")
            .and_then(Value::as_str)
            .ok_or("Unsupported grass appearance object")?;
        if ["grass", "swamp_grass"].contains(&map) {
            return Ok(Source::Map(map.into()));
        }
    }
    Err("Unsupported explicit grass color appearance".into())
}
pub fn load(resource: &mut Pack, behavior: &mut Pack, biome: &str) -> Result<Tint> {
    let name = identifier(biome)?;
    let server = behavior
        .read(&format!("biomes/{name}.biome.json"))
        .map_err(|e| e.to_string())?;
    let document = resources::json(&server.bytes)?;
    if document["minecraft:biome"]["description"]["identifier"] != biome {
        return Err("Biome definition identifier differs from requested biome".into());
    }
    let climate = &document["minecraft:biome"]["components"]["minecraft:climate"];
    let temperature = number(climate, "temperature")?;
    let downfall = number(climate, "downfall")?;
    let mut selected = Source::Map("grass".into());
    let mut client_origins = json!([]);
    let mut basis = "default_grass_map_inferred";
    match resource.read(&format!("biomes/{name}.client_biome.json")) {
        Ok(client) => {
            let document = resources::json(&client.bytes)?;
            let id = document["minecraft:client_biome"]["description"]["identifier"]
                .as_str()
                .ok_or("Missing client biome identifier")?;
            if id != name && id != biome {
                return Err("Client biome identifier differs".into());
            }
            client_origins = json!(client.origins);
            if let Some(appearance) =
                document["minecraft:client_biome"]["components"].get("minecraft:grass_appearance")
            {
                selected = source(&appearance["color"])?;
                basis = "explicit_client_grass_appearance";
            }
        }
        Err(ResourceError::Missing(_)) => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut map_origin = Value::Null;
    let mut map_hash = Value::Null;
    let mut point = Value::Null;
    let (rgb, kind) = match selected {
        Source::Fixed(rgb) => (rgb, "fixed".to_owned()),
        Source::Map(map) => {
            let asset = resource
                .read(&format!("textures/colormap/{map}.png"))
                .map_err(|e| e.to_string())?;
            let texture = Texture::decode(&asset.bytes, "png")?;
            if texture.width != 256 || texture.height != 256 {
                return Err("Grass prototype requires a 256 by 256 color map".into());
            }
            let [x, y] = coordinates(temperature, downfall)?;
            let offset = (usize::from(y) * 256 + usize::from(x)) * 4;
            let rgb = texture.rgba[offset..offset + 3].try_into().unwrap();
            point = json!([x, y]);
            map_origin = json!(asset.origins);
            map_hash = json!(texture.rgba_sha256());
            (rgb, format!("map:{map}"))
        }
    };
    resource.verify_unchanged().map_err(|e| e.to_string())?;
    behavior.verify_unchanged().map_err(|e| e.to_string())?;
    let provenance = json!({"biome":biome,"rgb":rgb,"color_source":kind,"selection_basis":basis,"coordinates":point,"temperature_bits":temperature.to_bits(),"downfall_bits":downfall.to_bits(),"climate":climate,"behavior_origins":server.origins,"client_origins":client_origins,"colormap_origins":map_origin,"colormap_rgba_sha256":map_hash,"resource_hashes":resource.source_hashes(),"behavior_hashes":behavior.source_hashes(),"scope":"Explicit resource files; inferred float32 climate sampling and alpha grass-side mask; no native visual parity or engine pack activation"});
    Ok(Tint { rgb, provenance })
}
/// Prototype terrain interpretation: alpha selects the grass-side tint mask;
/// zero-alpha dirt keeps its RGB. Top faces receive tint across the full image.
pub fn texel(pixel: [u8; 4], tint: [u8; 3], side_mask: bool) -> [u8; 3] {
    let alpha = if side_mask { u32::from(pixel[3]) } else { 255 };
    std::array::from_fn(|i| {
        (u32::from(pixel[i]) * ((255 - alpha) * 255 + alpha * u32::from(tint[i])) / (255 * 255))
            as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn climate_clamping_and_float32_coordinates() {
        assert_eq!(coordinates(0.8, 0.4).unwrap(), [50, 173]);
        assert_eq!(coordinates(-0.5, 2.0).unwrap(), [255, 255]);
        assert_eq!(coordinates(2.0, -1.0).unwrap(), [0, 255]);
        assert_eq!(coordinates(2.0, 2.0).unwrap(), [0, 0]);
        assert!(coordinates(f32::NAN, 0.5).is_err());
        assert!(coordinates(0.5, f32::INFINITY).is_err());
    }
    #[test]
    fn zero_alpha_dirt_is_preserved_and_grass_mask_is_tinted() {
        let tint = [0, 128, 255];
        assert_eq!(texel([100, 120, 140, 0], tint, true), [100, 120, 140]);
        assert_eq!(texel([100, 120, 140, 255], tint, true), [0, 60, 140]);
        assert_eq!(texel([255, 255, 255, 128], tint, true), [127, 191, 255]);
        assert_eq!(texel([100, 120, 140, 255], tint, false), [0, 60, 140]);
    }
    #[test]
    fn explicit_appearance_and_identifier_validation() {
        assert!(matches!(
            source(&json!("#90814d")).unwrap(),
            Source::Fixed([144, 129, 77])
        ));
        assert!(matches!(
            source(&json!({"color_map":"swamp_grass"})).unwrap(),
            Source::Map(_)
        ));
        for value in [
            json!("#ab"),
            json!("#ffffff00"),
            json!({"color_map":"unknown"}),
            json!({"color_map":"grass","extra":true}),
        ] {
            assert!(source(&value).is_err());
        }
        for id in [
            "plains",
            "minecraft:../plains",
            "minecraft:Plains",
            "other:plains",
        ] {
            assert!(identifier(id).is_err());
        }
    }
}
