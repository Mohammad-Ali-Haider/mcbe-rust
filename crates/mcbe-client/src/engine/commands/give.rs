use super::*;
pub fn run(client: &mut Client, args: &[String]) -> Result<Outcome> {
    let state = client.scene.engine.as_mut().ok_or("No playable world")?;
    let id = state.registry.find(&args[0]).map_err(|_| {
        format!(
            "Unknown item '{}'. Use Tab to complete registered item IDs.",
            args[0]
        )
    })?;
    let count = args.get(1).map_or(Ok(1), |s| s.parse::<u16>()).map_err(
        |_| "Count must be a positive whole number within the configured inventory capacity",
    )?;
    let stack = state.registry.catalog_stack(id);
    stack.validate_item(&state.registry)?;
    state.player.receive(stack, count)?;
    Ok(Outcome::message(format!(
        "Gave {count} {} ({})",
        state.registry.get(id).label,
        state.registry.get(id).id
    )))
}
pub fn complete(client: &Client) -> Vec<String> {
    client.scene.engine.as_ref().map_or_else(Vec::new, |s| {
        s.registry.blocks.iter().map(|b| b.id.clone()).collect()
    })
}
