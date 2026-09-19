# MCBE Reconstruction

**Current direction: an expandable Rust engine for your own blocky game.**
The user changed the goal on 2026-10-14; exact Minecraft cloning is no longer
required. Earlier Bedrock research is retained as a reusable reference.

**Checkpoint 45 is runnable now, building on Blockwork Version 1.0.** Double-click `Run-engine.cmd` for the
Blockwork GPU engine: actual local Minecraft block textures, native window
resolution, F4 wireframes, a compact HUD, player inventory and 2×2 crafting.
A/D and horizontal mouse direction are corrected. Shift sneaks, F toggles
collision flight, and N toggles noclip. E opens inventory; right-click splits
stacks and Shift-click moves them. JSON cuboid models include slabs, four
oriented stairs and a window frame with a real gap. The exported
`Geometry-demo.cmd` opens a prepared gallery.
Block definitions and four recipes are editable JSON. F5 saves slots and terrain
on a background worker while the game continues rendering.
F4 shows wireframes; F8 switches optimized/reference geometry.
F10/R/T travel and F9 reloads now prepare destinations on a worker; Escape cancels.
The current scene keeps rendering while movement and editing pause.
Streamed CPU meshes build on a worker; F2 compares async/sync scheduling.
The native loading screen now appears before terrain, save discovery and local
texture decoding run on a startup worker. Failed launches retain retry/close
controls. See [visible startup](docs/engine-startup.md).
The generated demo uses about 92% fewer triangles with merged faces.
It opens maximized; F11 toggles fullscreen and F7 changes GUI scale.
`New-engine-world.cmd` starts fresh with generated column streaming; F10 visits
distant terrain locations. Version 5 saves store the generator, sparse edits, movement mode, posture and models.
Older finite saves retain their existing world bounds. `Run-engine.cmd` continues the latest save.
See [Version 1 guide](docs/version-1.md), [custom JSON geometry](docs/custom-geometry.md),
[responsive travel and reloads](docs/engine-loading.md), [background saving](docs/engine-saving.md), [streaming and saves](docs/engine-streaming.md), [engine controls and source map](docs/engine-gpu.md) and
[visible checkpoint history](docs/engine-checkpoints.md).

The engine is still in development. The following sections document the earlier
research client and its optional captured-world workflows.

**Status: playable engine foundation; further game systems remain unfinished.**

**Rust is the target language for the reconstructed game.** The Cargo workspace
now contains a native core and CLI. Existing Python models remain as research
references while their remaining systems migrate. Start with `docs/rust-rewrite.md`.

## Run

The default executable and `Run-client.cmd` now open the standalone engine.
For the earlier captured stone scene, use `Run-research-client.cmd`. That mode
supports walking, looking, test jumping, and cube edits. G cycles the available
source-world materials. `Run-terrain-study.cmd` opens the stone/ore research
fixture with 27 additional exact materials. F5 saves a new world
copy; F9 reloads this session's latest copy. See
`docs/rust-client.md` for controls and current limits.

Press I for the inventory study panel, or open `Run-inventory-study.cmd`.
Its barrel/hopper/dispenser operations use the new native-verified Rust container
model. F5/F9 now save/reload copied-world container contents, with 37 source-backed
item types available. See `docs/rust-inventory.md` and `docs/rust-container-storage.md`
for controls, native acceptance evidence and remaining player integration.

Checkpoint 34 adds native-checked container collision and targeting geometry in
the Rust core. Walking-view container interaction remains to be connected; see
`docs/rust-container-geometry.md` for the exact supported states and known gaps.

`Run-container-world.cmd` opens the checkpoint 35 walking fixture. Look at a
barrel, hopper or dispenser and press E or right-click to edit its actual saved
contents. F5/F9 save/reload block and container edits in one shared world copy.
Container visuals remain provisional. See `docs/rust-container-world.md`.

Build and run the Rust implementation from this folder (tested with Rust 1.94.1):

```powershell
cargo test --workspace
cargo run --release -p mcbe-cli -- demo
```

The demo prints collision trajectories; it is not a playable client. The Rust
executable does not require Python. The retained Python 3.12+ research tools use
the standard library; optional native tools use local Capstone/Unicorn and Ghidra/JDK:

```powershell
python -m unittest discover -s tests -v
python -m mcbe capture --root 'C:\XboxGames\Minecraft for Windows\Content' --out 'evidence\new-run'
python -m mcbe archive 'C:\path\recipes.brarchive'
python -m mcbe archive 'C:\path\recipes.brarchive' --extract 'local-source\recipes'
python tools/verify_capture.py evidence/stable-1.26.5203.0-data
python tools/validate_recipes.py evidence/stable-1.26.5203.0-data
```

Use a new output folder for each capture. Capture/archive commands read files and
write research output. The separate reference-probe tool launches an isolated
server copy; see `docs/reference-runtime.md` before running an experiment.

## Study order

1. `docs/status.md`: scope, completed evidence, missing systems, next actions.
2. `docs/archive-format.md`: what the archive reader reconstructs.
3. `evidence/<run>/target.json` and `native.json`: exact target identity and PE facts.
4. `evidence/<run>/summary.json`: dataset coverage and any errors.
5. `evidence/<run>/archive-index.jsonl`: archive entries, offsets, hashes, payload type.
6. `evidence/<run>/local-extracted/<pack>/recipes`: original local recipe definitions.
7. `docs/recipes.md` and `mcbe/recipes.py`: first crafting implementation and its limits.
8. `docs/reference-runtime.md`: live server experiments and measured item-stack behavior.
9. `mcbe/registry.py`: registry loader and original item-stack implementation.
10. `mcbe/`: original implementation source; `tests/`: independently constructed fixtures.
11. `docs/native-recipes.md`: focused decompilation and the native-derived symmetry fix.
12. `docs/chunk-storage.md`: offline database/subchunk reader and saved-world comparisons.
13. `docs/world-writing.md`: independent serialization and native acceptance of saved block edits.
14. `docs/heightmaps.md`: separate column-height/topmost rules and native-verified height repairs.
15. `docs/waterlogging.md`: measured two-layer cells and native-verified waterlogging edits.
16. `docs/raycast.md`: block targeting geometry, boundary behavior, and independent native checks.
17. `docs/world-targeting.md`: targeting actual saved blocks and trying the read-only ray command.
18. `docs/vertical-motion.md`: original falling/landing model and independent native trajectories.
19. `docs/planar-motion.md`: horizontal friction, honey slowing, slime rebound, and native arithmetic.
20. `docs/collision.md`: solid walls/ceilings, sliding, platforms, and measured step-up alternatives.
21. `docs/world-collision.md`: saved-world block collision and independent native reload checks.
22. `docs/rust-rewrite.md`: Rust source map, executable commands, migration checks, and remaining ports.
23. `docs/rust-world-storage.md`: direct Rust database/subchunk loading and saved-world movement.
24. `docs/rust-blocks.md`: ordered block properties, immutable assignments, and saved-block inspection in Rust.
25. `docs/rust-targeting.md`: Rust ray queries against saved worlds, measured selection shapes, and evidence.
26. `docs/rust-world-writing.md`: chunk serialization, block-copy command, and verified world-copy writing in Rust.
27. `docs/rust-heightmaps.md`: measured height/topmost rules and offline Rust repair commands.
28. `docs/rust-waterlogging.md`: stored waterlogging, copied-world edits, and integrated height repair in Rust.
29. `docs/rust-items.md`: measured registry, pristine stacks, and constructor/setter differences in Rust.
30. `docs/rust-recipes.md`: shaped/shapeless candidate matching and definition coverage in Rust.
31. `docs/rust-client.md`: launch the native development window and inspect its integration evidence.
32. `docs/rust-resources.md`: packed/loose resource lookup, texture decoding, and material provenance.
33. `docs/cube-world.md`: native four-material collision, expanded terrain view, and typed saves.
34. `docs/rust-biomes.md`: native biome identities, packed palettes, and top-slice extension.
35. `docs/rust-grass.md`: provisional climate-based coloring and grass-side masks.
36. `docs/rust-streaming.md`: movable column residency, eviction-safe edits and native save acceptance.
37. `docs/rust-materials.md`: 31 supported cube states, stone/ore study launcher, explicit texture packs and fresh native evidence.
38. `docs/rust-inventory.md`: native container operations, transfer remainders and the interactive inventory study panel.
39. `docs/rust-container-storage.md`: lossless saved container records, source-backed item serialization and native-accepted client saves.

## Current checkpoint

The Rust development client now follows the actor with a movable saved-column cache.
Native-window and headless checks pass targeting, placement/removal, collision,
test jumping, copied-world saves/reload and source preservation. A fresh native
reload matches 2,028 cells and 256 topmost queries from the client-written save.
Its controls and lighting remain development implementations; the stone texture
now loads from the local base vanilla pack. Native player and visual parity remain open. See
`docs/rust-client.md`, `docs/rust-streaming.md`, `evidence/streaming-03/summary.json`, and
`evidence/client-native-01/coverage-audit/summary.json`.

98 Python research tests and 69 Rust workspace unit tests pass. All 1,841 captured archives and 33,464 entries were independently
verified against the installation. Using measured item tags, recipe-definition
checks cover 3,196 of 3,822 definitions; these are not live crafting parity tests.

A separate official Bedrock Dedicated Server 1.26.52.3 reference produced 1,623
registered items, 1,477 blocks, and 77 item tags in two identical registry captures.
The original item-stack model matches 147 recorded operations and 116 additional
operations recorded after freezing the model. See `docs/reference-runtime.md` for
scope, evidence, and reproduction commands. All 597 client behavior archives have
byte-identical counterparts in this server distribution.

REA 6.4.0, Ghidra 12.1.4, and portable JDK 21 are installed under `tools/vendor`.
The first REA whole-import analysis timed out. A subsequent focused Ghidra run
successfully decompiled one recipe-loading function. Bounded emulation of its
symmetry flag path found and verified a correction to the Python implementation.
See `docs/native-recipes.md` for the remaining type uncertainties and test limits.
The client executable remains unreadable through the tested Windows paths.

World metadata serialization now preserves six reference files byte for byte.
The server accepted a file rewritten by our codec and retained its edited time
and daylight-cycle settings across two startups. See `docs/world-metadata.md` and
`mcbe/nbt.py`. The new read-only database/subchunk implementation matches 4,114
placed blocks and 5,734 observations after an independent restart, including
1,620 previously unobserved terrain/air positions. Persistent/API state differences
are converted explicitly using the measured palette. An experimental world-copy
writer now produces edited subchunks accepted by the server: 13,924 predicted
blocks match native observations and the resaved data. A measured heightmap model
now repairs 561 stale entries from those block edits; all 2,304 repaired-world
column predictions pass native checks. Lighting and other save invariants remain
open. See `docs/chunk-storage.md`, `docs/world-writing.md`, and `docs/heightmaps.md`.

Two-layer waterlogged cells now have measured read/edit support. Independently
written wet/dry reversals pass all 54 native cell checks, exact saved-layer
comparisons, and 1,024 height checks. Fluid simulation remains open; see
`docs/waterlogging.md` for exact-state coverage and limitations.

Finite-scene block targeting now covers 27 measured states. After correcting
distance boundaries, stair selection boxes, and float32 arithmetic, a frozen
model passes 4,185 fresh native queries. Hit coordinates pass within the declared
0.00002 tolerance. Movement collision and liquid targeting remain open; see
`docs/raycast.md` for retained failed experiments and reproduction commands.

Targeting is now connected to saved world data through a lazy block reader. An
independent reload comparison passes 2,970 rays and 10,855 sampled cells; source
and resaved databases reproduce the same predictions. `python -m mcbe ray` lets
you inspect hits without running the game. See `docs/world-targeting.md` for a
working example and the additional coordinate-rounding corrections.

An original vertical actor-motion model now separates internal velocity from
observed displacement and reproduces falling/landing on six measured surfaces.
All 10,860 independently frozen frames from 60 new custom-actor/armor-stand
trajectories pass. This is not a player controller or general collision solver;
see `docs/vertical-motion.md` for scope and a runnable study example.

Uniform-surface actor motion now includes horizontal friction, honey slowing,
and slime rebound on five measured surface states. A frozen model passes all
16,870 fresh frames across 70 trajectories. A separate bounded native check
matches 21 slowing-arithmetic cases bit for bit. This still uses a caller-supplied
ground plane; see `docs/planar-motion.md` for a runnable example, retained failures,
sampling-depth corrections, and remaining collision/input work.

A separate solid-box solver now handles wall/ceiling impacts, sliding, platform
landings, and measured step-up alternatives in stone scenes. A frozen model
passes all 15,928 fresh frames across 88 trajectories after step-path and rounding
corrections. Boxes are supplied by the caller; world collision geometry, material
effects, and player controls still need integration. See `docs/collision.md`.

The Python world reader now supplies collision boxes from saved stone/air cells.
A fresh native reload passes 15,928 frames and 16,467 cell comparisons; frozen
source and resaved world predictions agree. See `docs/world-collision.md`.

The initial Rust port passes 48,726 existing native motion frames, all 1,841
archive identities and 33,464 entry/payload checks, and six byte-exact metadata
round trips. The Cargo workspace builds a release executable; format checks and
Clippy with warnings denied pass. Rust now reads saved-world databases directly:
four snapshots match 2,267 logical entries and 392 subchunks. Original/resaved
world collision each matches 15,928 retained native frames and 16,467 cells.
These replay existing observations; remaining subsystems still need migration.
See `docs/rust-rewrite.md` for precise coverage.

Rust block-state handling now passes all 69,223 full mappings, 22,866 partial
operations, and 24,336 previously held-out operations. Saved-state normalization
passes 4,114 initial cells and 5,734 reload cells against both source and resaved
worlds. New `block` and `block-at` commands expose the implementation for study.
See `docs/rust-blocks.md`.

Rust targeting passes 4,185 finite-scene rays and 2,970 saved-world rays from
existing native captures. Original/resaved snapshots each match 10,855 sampled
cells and the exact traversed-cell set. The new native
`ray` command queries stopped worlds; see `docs/rust-targeting.md`.

Rust world-copy writing now recreates the earlier native-accepted world byte for
byte and matches 13,924 retained native block observations. Its checkpoint has
24 passing checks plus five writer checks, including 392 exact chunk round trips.
`copy-block` creates a new edited world without modifying its source. Lighting
and simulation updates remain open; see `docs/rust-world-writing.md`.

Rust height repair corrects 561 stale entries across three chunks, reproducing
the native-accepted repaired world byte for byte. All 26 height-checkpoint release checks
and four repair checks pass. Inspect with `heights` or repair a separate world
copy with `repair-heights`; see `docs/rust-heightmaps.md`. Block-copy edits do
not automatically repair heights.

Rust waterlogging now matches 54 retained operation calls and recreates 42
wet/dry edits byte for byte. All 54 edited cells and 1,024 heights match retained
native evidence. All 28 waterlogging-checkpoint release checks and six waterlogging checks pass.
The `waterlog` command repairs touched heights automatically; fluid simulation
remains open. See `docs/rust-waterlogging.md`.

Rust now loads the item registry and reproduces pristine stack construction,
amount assignment, and compatibility. All 147 original cases and 116 later
cases pass, plus the earlier 60 constructor cases and repeated registry.
The item checkpoint has 31 passing checks. Try the `item` command; see
`docs/rust-items.md`. Inventory transfer and metadata mutation remain open.

Rust shaped/shapeless candidate matching now covers the same 3,196 supported
definitions out of 3,822 captured files, with identical exclusions. All 15 retained
native symmetry cases pass. The recipe checkpoint has 33 passing checks. The
`recipe` command exposes grid queries; see `docs/rust-recipes.md`. Live crafting,
inventory consumption, and effective pack selection remain open.

Rust resource lookup and PNG/TGA decoding now match all 703 base block images
against independent Pillow output. The client loads the installed stone texture
through its block and terrain aliases. The resource checkpoint has 34 passing checks.
See `docs/rust-resources.md` for explicit lookup rules and remaining material work.

Four-material saved-world collision now passes 10,304 fresh native frames and
7,888 sampled cells. The client region has expanded to 49,152 logical cells;
press T to inspect its ground. Typed terrain edits, including a new subchunk,
pass a fresh native reload across that entire region and 1,024 topmost queries.
Saved biome decoding now matches 7,776 fresh native queries across 12 identities;
see `docs/rust-biomes.md`. All 56 current release checks pass. The client now uses
a provisional climate/color-map grass model, checked independently across 71
explicit biome definitions; see `docs/rust-grass.md`. Native visual and player
input parity remain open.

Block-state handling now loads 69,223 observed input combinations into 22,079
canonical permutations. Separate resolve/assignment measurements account for
legacy state coupling. The frozen implementation passes 24,336 fresh comparison
cases; see `docs/block-states.md` for evidence, known exceptions, and limitations.

The current capture is `evidence/stable-1.26.5203.0-data`. Its archive data is
available, but `native.json` records Windows denying executable reads. The capture
command exits nonzero when any component is unavailable; inspect `summary.json`
to distinguish a partial capture from complete success.

The original game files retain their owners' rights. Local reference captures are
excluded from version control. Keep source provenance with all findings.
The tools do not establish permission to redistribute game content.

## Verification levels

- **Observed**: bytes, offsets, hashes, or local file contents actually inspected.
- **Documented**: claims supported by a linked specification or developer reference.
- **Inferred**: plausible interpretation that still needs an engine experiment.
- **Behaviorally verified**: original and reconstructed implementations tested with
  the same inputs and compared. No whole-game parity claim is currently made.
