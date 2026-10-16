//! Explicit local Minecraft resource bindings; decoded once, outside the frame loop.
use crate::{
    engine_blocks::{BlockId, Registry},
    scene::Result,
};
use mcbe_core::{
    resources::Pack,
    textures::{Material, Texture},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};
pub const DEFAULT_ROOT: &str = r"C:\XboxGames\Minecraft for Windows\Content\data\resource_packs";
pub struct Assets {
    pub inventory: BTreeMap<&'static str, Texture>,
    pub root: PathBuf,
    pub materials: HashMap<(BlockId, usize), Arc<Material>>,
    pub hotbar: Vec<Texture>,
    pub selected: Texture,
    pub start_cap: Texture,
    pub end_cap: Texture,
    pub icons: Texture,
    pub font: Texture,
    pub panel: Texture,
    pub cell: Texture,
    pub grass_tint: [u8; 3],
    pub provenance: Value,
}
fn texture(pack: &mut Pack, path: &str) -> Result<Texture> {
    Ok(Texture::decode(&pack.read(path)?.bytes, "png")?)
}
pub fn load(registry: &Registry, root: &Path) -> Result<Assets> {
    load_inner(registry, root, false)
}
pub fn load_for(registry: &Registry, root: &Path, legacy_ui: bool) -> Result<Assets> {
    if legacy_ui {
        load_inner(registry, root, true)
    } else {
        load(registry, root)
    }
}
fn load_inner(registry: &Registry, root: &Path, legacy_ui: bool) -> Result<Assets> {
    let mut packs = HashMap::new();
    let mut materials = HashMap::new();
    let mut bindings = Vec::new();
    let mut decoded = HashMap::<(String, String, String), Arc<Material>>::new();
    for (index, b) in registry.blocks.iter().enumerate() {
        if b.minecraft.is_some() || b.face_textures.is_some() {
            for (face, name) in ["up", "down", "north", "south", "east", "west"]
                .iter()
                .enumerate()
            {
                let binding = b
                    .face_textures
                    .as_ref()
                    .map(|f| &f[face])
                    .or(b.minecraft.as_ref())
                    .unwrap();
                if !packs.contains_key(&binding.pack) {
                    packs.insert(binding.pack.clone(), Pack::open(&root.join(&binding.pack))?);
                }
                let pack = packs.get_mut(&binding.pack).unwrap();

                let face_name = binding.face.as_deref().unwrap_or(name);
                let key = (
                    binding.pack.clone(),
                    binding.block.clone(),
                    face_name.to_owned(),
                );
                let material = if let Some(m) = decoded.get(&key) {
                    m.clone()
                } else {
                    let m = Arc::new(mcbe_core::textures::block_material(
                        pack,
                        &binding.block,
                        face_name,
                        0,
                    )?);
                    decoded.insert(key, m.clone());
                    m
                };
                if b.render_layer == crate::engine_blocks::RenderLayer::Opaque
                    && binding.block != "grass"
                    && material.texture.rgba.chunks_exact(4).any(|p| p[3] != 255)
                {
                    return Err(
                        format!("Opaque cube texture required: {} / {name}", b.label).into(),
                    );
                }
                bindings.push(json!({"id":b.id,"face":name,"material":material.provenance}));
                materials.insert((index as BlockId, face), material);
            }
        }
    }
    if !packs.contains_key("vanilla") {
        packs.insert("vanilla".into(), Pack::open(&root.join("vanilla"))?);
    }
    let base = packs.get_mut("vanilla").unwrap();
    if !legacy_ui {
        let behavior_root = root
            .parent()
            .ok_or("Resource root has no parent")?
            .join("behavior_packs/vanilla");
        let mut behavior = Pack::open(&behavior_root)?;
        let tint = mcbe_core::grass::load(base, &mut behavior, "minecraft:plains")?;
        behavior.verify_unchanged()?;
        let mut hashes = BTreeMap::new();
        for pack in packs.values() {
            pack.verify_unchanged()?;
            hashes.extend(
                pack.source_hashes()
                    .iter()
                    .map(|(p, h)| (p.to_string_lossy().into_owned(), h.clone())),
            );
        }
        hashes.extend(
            behavior
                .source_hashes()
                .iter()
                .map(|(p, h)| (p.to_string_lossy().into_owned(), h.clone())),
        );
        let white = || Texture {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        return Ok(Assets {
            inventory: BTreeMap::new(),
            root: root.to_path_buf(),
            materials,
            hotbar: vec![],
            selected: white(),
            start_cap: white(),
            end_cap: white(),
            icons: white(),
            font: white(),
            panel: white(),
            cell: white(),
            grass_tint: tint.rgb,
            provenance: json!({"source_root":root,"source_hashes":hashes,"bindings":bindings,"scope":"Read-only block textures and grass tint; game UI uses authored engine layouts, colors, font and sprites"}),
        });
    }
    let hotbar = (0..9)
        .map(|i| texture(base, &format!("textures/ui/hotbar_{i}.png")))
        .collect::<Result<Vec<_>>>()?;
    let selected = texture(base, "textures/ui/selected_hotbar_slot.png")?;
    let start_cap = texture(base, "textures/ui/hotbar_start_cap.png")?;
    let end_cap = texture(base, "textures/ui/hotbar_end_cap.png")?;
    let icons = texture(base, "textures/gui/icons.png")?;
    let font = texture(base, "font/default8.png")?;
    let panel = texture(base, "textures/ui/dialog_background_opaque.png")?;
    let cell = texture(base, "textures/ui/item_cell.png")?;
    let mut inventory = BTreeMap::new();
    let mut inventory_textures = Vec::new();
    for (key, path) in [
        ("helmet", "textures/ui/empty_armor_slot_helmet.png"),
        ("chestplate", "textures/ui/empty_armor_slot_chestplate.png"),
        ("leggings", "textures/ui/empty_armor_slot_leggings.png"),
        ("boots", "textures/ui/empty_armor_slot_boots.png"),
        ("shield", "textures/ui/empty_armor_slot_shield.png"),
        ("arrow", "textures/ui/arrow.png"),
        ("skin", "textures/entity/steve.png"),
        ("preview_border", "textures/ui/player_preview_border.png"),
        ("fold", "textures/ui/recipe_back_panel.png"),
    ] {
        let source = base.read(path)?;
        let decoded = Texture::decode(&source.bytes, "png")?;
        inventory_textures.push(json!({"key":key,"path":path,"origins":source.origins,"width":decoded.width,"height":decoded.height,"rgba_sha256":format!("{:x}",Sha256::digest(&decoded.rgba))}));
        inventory.insert(key, decoded);
    }
    let model_source = base.read("models/mobs.json")?;
    let model_json = mcbe_core::resources::json(&model_source.bytes)?;
    let skin = &inventory["skin"];
    let preview = crate::engine_preview::Model::parse(&model_json, [skin.width, skin.height])?;
    let preview_provenance = json!({"path":"models/mobs.json","origins":model_source.origins,"cubes":preview.cubes,"model":"geometry.humanoid.custom:geometry.humanoid","skin_size":[skin.width,skin.height]});
    let common_source = base.read("ui/ui_common.json")?;
    let common = mcbe_core::resources::json(&common_source.bytes)?;
    let inventory_source = base.read("ui/inventory_screen.json")?;
    let inv = mcbe_core::resources::json(&inventory_source.bytes)?;
    if common["root_panel"]["size"] != json!([176, 166])
        || common["hotbar_grid_template"]["size"] != json!([162, 18])
        || inv["crafting_grid_2x2"]["size"] != json!([36, 36])
        || inv["recipe_inventory_screen_content"]["controls"][0]["content_stack_panel"]["size"]
            != json!([326, 166])
    {
        return Err("Installed desktop inventory layout changed; inspect its dimensions".into());
    }
    let hud = base.read("ui/hud_screen.json")?;
    let hud_json = mcbe_core::resources::json(&hud.bytes)?;
    if hud_json["gui_hotbar_grid_item"]["size"] != json!([20, 22])
        || hud_json["hotbar_slot_selected_image"]["size"] != json!([24, 24])
    {
        return Err("Installed HUD geometry changed; inspect the new layout".into());
    }
    let behavior_root = root
        .parent()
        .ok_or("Resource root has no parent")?
        .join("behavior_packs/vanilla");
    let mut behavior = Pack::open(&behavior_root)?;
    let tint = mcbe_core::grass::load(base, &mut behavior, "minecraft:plains")?;
    behavior.verify_unchanged()?;
    let mut source_hashes = BTreeMap::new();
    for pack in packs.values() {
        pack.verify_unchanged()?;
        source_hashes.extend(
            pack.source_hashes()
                .iter()
                .map(|(p, h)| (p.to_string_lossy().into_owned(), h.clone())),
        );
    }
    source_hashes.extend(
        behavior
            .source_hashes()
            .iter()
            .map(|(p, h)| (p.to_string_lossy().into_owned(), h.clone())),
    );
    let provenance = json!({"preview_model":preview_provenance,"source_root":root,"source_hashes":source_hashes,"bindings":bindings,"hud_origins":hud.origins,"hotbar_slot_size":[20,22],"selection_size":[24,24],"inventory_textures":inventory_textures,"inventory_origins":inventory_source.origins,"common_origins":common_source.origins,"desktop_panel_size":[176,166],"desktop_content_size":[326,166],"inventory_slot_size":[18,18],"crafting_grid_size":[36,36],"grass_biome":"minecraft:plains","scope":"Exact decoded local texture pixels and inspected HUD/inventory dimensions; full native lighting, model UVs, live 3D player preview and interface behavior require separate visual checks"});
    Ok(Assets {
        inventory,
        root: root.to_path_buf(),
        materials,
        hotbar,
        selected,
        start_cap,
        end_cap,
        icons,
        font,
        panel,
        cell,
        grass_tint: tint.rgb,
        provenance,
    })
}
impl Assets {
    pub fn color(
        &self,
        registry: &Registry,
        id: BlockId,
        face: usize,
        u: f64,
        v: f64,
    ) -> Option<[f64; 3]> {
        let material = self.materials.get(&(id, face))?;
        let texel = material
            .texture
            .sample_repeat(u, if face <= 1 { v } else { 1. - v });
        let grass = registry
            .get(id)
            .minecraft
            .as_ref()
            .is_some_and(|b| b.block == "grass");
        let rgb = if grass && face != 1 {
            mcbe_core::grass::texel(texel, self.grass_tint, face != 0)
        } else {
            [texel[0], texel[1], texel[2]]
        };
        Some(crate::engine_materials::tint(rgb, registry.get(id).texture_tint).map(f64::from))
    }
}
