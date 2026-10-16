use super::*;
pub fn run(client: &mut Client, args: &[String]) -> Result<Outcome> {
    // Validate IDs, free camera and unsettled crafting before scheduling.
    // The real immutable snapshot is captured once by the shared loading service.
    let state = client.scene.engine.as_ref().ok_or("No playable world")?;
    let dimensions = state
        .dimensions
        .as_ref()
        .ok_or("Dimensions are unavailable in this world")?;
    let definition = dimensions.definitions.get(&args[0])?;
    if dimensions.active == definition.id {
        return Err(format!("Already in dimension {}", definition.id).into());
    }
    if client.view.is_free() {
        return Err("Return from free camera before dimension travel".into());
    }
    let p = &state.player;
    if p.cursor.is_some() || p.crafting.iter().chain(&p.table).any(Option::is_some) {
        return Err("Close crafting and settle held items before dimension travel".into());
    }
    Ok(Outcome {
        message: format!("Preparing {} ({})...", definition.label, definition.id),
        travel: Some(definition.id.clone()),
    })
}
pub fn complete(client: &Client) -> Vec<String> {
    client
        .scene
        .engine
        .as_ref()
        .and_then(|s| s.dimensions.as_ref())
        .map_or_else(Vec::new, |d| {
            d.definitions
                .dimensions
                .iter()
                .map(|d| d.id.clone())
                .collect()
        })
}
