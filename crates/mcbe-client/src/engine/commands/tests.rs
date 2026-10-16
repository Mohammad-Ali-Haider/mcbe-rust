use super::*;
use crate::{
    engine,
    engine_inventory::{Inventory, Stack},
};
fn probe(_: &mut Client, args: &[String]) -> Result<Outcome> {
    Ok(Outcome::message(args.join("|")))
}
#[test]
fn argument_parser_and_registration_accept_quoted_arguments_without_shell_evaluation() {
    let mut c = engine::generate(37).unwrap();
    let mut r = Registry::built_in();
    r.register(
        "probe",
        Command {
            usage: "/probe <a> <b>",
            arguments: 2..=2,
            run: probe,
            complete: |_| vec![],
        },
    )
    .unwrap();
    assert!(
        r.register(
            "probe",
            Command {
                usage: "",
                arguments: 0..=0,
                run: probe,
                complete: |_| vec![]
            }
        )
        .is_err()
    );
    assert_eq!(
        r.execute(&mut c, r#"/probe "two words" '$(literal)'"#)
            .unwrap()
            .message,
        "two words|$(literal)"
    );
    for line in [
        "/probe one",
        "/probe \"unfinished",
        "/probe a b c",
        "/unknown",
        "/give",
    ] {
        assert!(r.execute(&mut c, line).is_err(), "{line}");
    }
    assert!(arguments(&"x".repeat(513)).is_err());
    assert!(arguments("a\nb").is_err());
    assert_eq!(r.complete(&c, "/tr"), vec!["/travel "]);
    assert!(
        r.complete(&c, "/give gla")
            .iter()
            .any(|s| s == "/give glass ")
    );
}
#[test]
fn give_merges_splits_respects_container_limits_and_rolls_back_on_overflow() {
    let mut c = engine::generate(37).unwrap();
    let r = Registry::built_in();
    let s = c.scene.engine.as_mut().unwrap();
    s.player = Inventory::empty();
    let glass = s.registry.find("glass").unwrap();
    s.player.slots[0] = Some(Stack {
        id: glass,
        count: 60,
        data: None,
    });
    r.execute(&mut c, "/give glass 70").unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(
        s.player.slots[..3]
            .iter()
            .flatten()
            .map(|s| s.count)
            .collect::<Vec<_>>(),
        vec![64, 64, 2]
    );
    r.execute(&mut c, "/give bucket 2").unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    let bucket = s.registry.find("bucket").unwrap();
    let stacks: Vec<_> = s
        .player
        .slots
        .iter()
        .flatten()
        .filter(|s| s.id == bucket)
        .collect();
    assert_eq!(stacks.len(), 2);
    assert!(stacks.iter().all(|s| s.count == 1 && s.data.is_some()));
    s.player.validate_items(&s.registry).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    s.player.slots.fill(Some(Stack {
        id: glass,
        count: 64,
        data: None,
    }));
    s.player.slots[0].as_mut().unwrap().count = 63;
    let before = s.player.clone();
    for line in [
        "/give glass 2",
        "/give missing 1",
        "/give glass 0",
        "/give glass -1",
        "/give glass 2305",
        "/give glass 1 extra",
    ] {
        assert!(r.execute(&mut c, line).is_err());
        assert_eq!(
            c.scene
                .engine
                .as_ref()
                .unwrap()
                .player
                .changed_slots(&before),
            0
        );
        assert_eq!(
            c.scene.engine.as_ref().unwrap().player.revision,
            before.revision
        );
    }
}
#[test]
fn travel_resolves_the_saved_registry_and_rejects_invalid_state_without_mutation() {
    let mut c =
        crate::engine_stream::generate(37, crate::engine_blocks::Registry::built_in()).unwrap();
    engine::world::dimensions::attach_legacy(
        &mut c,
        engine::world::dimensions::Definitions::built_in(),
    )
    .unwrap();
    let r = Registry::built_in();
    let instance = c.scene.engine.as_ref().unwrap().instance;
    assert!(r.execute(&mut c, "/travel surface").is_err());
    assert!(r.execute(&mut c, "/travel unknown").is_err());
    assert_eq!(
        r.execute(&mut c, "/travel cavern")
            .unwrap()
            .travel
            .as_deref(),
        Some("cavern")
    );
    assert_eq!(c.scene.engine.as_ref().unwrap().instance, instance);
    c.view.mode = engine::player::camera::Mode::Free;
    assert!(r.execute(&mut c, "/travel islands").is_err());
}
