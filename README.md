# Minecraft Rust Edition

An expandable Rust voxel engine for your own block game. This is the single
active project; development versions now live in Git history.

## Screenshots

![Forest terrain with textured blocks, chunk statistics and the hotbar](docs/media/terrain.png)

Terrain exploration in the Forward renderer. The gallery uses actual renderer
captures; the terrain and wireframe images are earlier verification scenes, while
the inventory image uses the current source. HUD FPS is an instantaneous sample.

| Inventory and GPU player preview | F4 triangle wireframe |
|---|---|
| ![Original inventory UI with item search, crafting, storage and player preview](docs/media/inventory.png) | ![Island terrain with its actual triangle edges highlighted](docs/media/wireframe.png) |

## Performance and triangle reduction

One recorded **current-source** production GPU fixture represents **6,557 exposed
faces with 2,452 greedy quads**: **62.6% fewer quads and terrain vertex bytes** than
separate exposed faces. Its cached nearby geometry is:

| Metric | Separate exposed faces (calculated) | Greedy mesh (captured) |
|---|---:|---:|
| Quads | 6,557 | 2,452 |
| Triangles | 13,114 | 4,904 |
| Terrain vertex payload | 1,573,680 bytes | 588,480 bytes |

The same frame reports **275,263 internal faces omitted** and **7 of 21 cached
nearby chunks drawn**, submitting **1,758 nearby triangles** after view/range
tests. These counts exclude distant LOD, actors and UI. The fixture ran offscreen
at 512 x 288 on an NVIDIA GeForce RTX 5070 Ti Laptop GPU through DX12; the
reference column is derived from its counters, not a fresh A/B timing run.

An earlier finite-court A/B benchmark reduced **39,172 to 3,212 triangles
(91.8%)**, with **3.7% higher measured event/present throughput** in that small
scene. Geometry savings and frame-rate gains are different measurements. See
[performance evidence and reproduction](docs/PERFORMANCE.md) for the raw numbers,
run conditions, historical timing results and measurement limits.

### How the engine saves work

| Algorithm / system | What it does | Implementation |
|---|---|---|
| Hidden-face removal | Tests neighboring block shapes and emits exposed surfaces, including across chunk boundaries. | [Shapes](crates/mcbe-client/src/engine/blocks/shapes.rs), [mesher](crates/mcbe-client/src/engine/render/mesh.rs) |
| Greedy rectangle meshing | Sweeps six directions through each 16-cubed chunk; combines compatible full opaque/cutout faces of the same block state. Texture coordinates repeat per block. | [Mesh construction](crates/mcbe-client/src/engine/render/mesh.rs) |
| Frustum and distance culling | Rejects chunk bounds outside the camera or range before submitting terrain draws. GPU back-face culling and depth testing handle surface visibility. | [Bounds tests](crates/mcbe-client/src/engine/render/mesh.rs), [draw selection](crates/mcbe-client/src/engine/render/gpu.rs) |
| Dirty mesh caching | Reuses uploaded chunk buffers and rebuilds affected/version-changed meshes instead of rebuilding the world every frame. | [GPU mesh cache](crates/mcbe-client/src/engine/render/gpu.rs) |
| Bounded asynchronous meshing | A worker builds owned 18-cubed halo snapshots; at most two jobs are in flight. Version/epoch checks reject stale work, and accepted uploads are paced. | [Worker scheduler](crates/mcbe-client/src/engine/render/meshing.rs) |
| Adaptive distant terrain | Recursively subdivides terrain patches with distance and boundary constraints; stitches shared edges and retains coarser geometry for far views. | [LOD generation](crates/mcbe-client/src/engine/render/distance.rs) |
| Readiness-based LOD handoff | Keeps fallback terrain until detailed GPU chunks are ready, with 240 ms detail fades and 180 ms distant-mesh crossfades. | [Coverage](crates/mcbe-client/src/engine/render/coverage.rs), [LOD cache](crates/mcbe-client/src/engine/render/distance.rs) |
| Compact chunk storage | Allocates dense chunks sparsely, stores block IDs in 16 bits and shares chunk snapshots with copy-on-write. A 16-cubed block-ID payload is 8 KiB before overhead. | [Voxel storage](crates/mcbe-client/src/engine/world/storage.rs) |
| Reused textures, instances and UI | Caches mipmapped textures and shared creature cuboids; uploads settled UI/player previews only when their inputs change. | [Textures](crates/mcbe-client/src/engine/render/texture.rs), [entities](crates/mcbe-client/src/engine/entities/), [UI renderer](crates/mcbe-client/src/engine/render/gpu_ui.rs) |
| Incremental optional ray scene | Reuses mesh-part hashes and vertex ranges; rebuilds at most two BLAS parts per frame within quality budgets. Forward remains the default. | [Ray renderer](crates/mcbe-client/src/engine/render/methods/ray/) |

For an **isolated, single-material solid 16 x 16 x 16 chunk**, independent cubes
would use 49,152 triangles. Removing internal faces leaves 3,072; merging its six
outer surfaces leaves just **12 triangles**. This is an idealized example, not a
whole-world benchmark. Partial blocks and translucent faces follow their own
geometry/sorting paths and do not all merge into those rectangles.

**Inspect it yourself:** F3 shows rendering counters, F4 toggles triangle edges,
F8 switches greedy/reference meshing, and F2 switches asynchronous/synchronous
meshing. Chunk culling currently uses camera/range bounds rather than GPU
occlusion queries or hierarchical-Z. See [Rendering](docs/RENDERING.md) for the
Forward, Deferred and hardware-ray tradeoffs.

## Open and run manually

Open `D:\Recontruction Projects\Minecraft Rust Edition` with VS Code → File → Open Folder.
Open Terminal → New Terminal, then run:

```powershell
cargo run --release
```

Cargo compiles changed Rust code in release mode and starts the game, loading
editable assets from this folder and continuing its latest saved world. With no
save, it creates a generated world. The window opens maximized; F11 toggles
borderless fullscreen. Close the game before compiling another build.

Plain `cargo run` also starts the game, using a slower debug build. Both commands
use this folder’s assets and continue the latest save. No launcher script is needed.

The complete explicit command is:

```powershell
cargo run --release -p mcbe-client -- --engine --engine-home . --engine-continue
```

Other useful commands:

```powershell
cargo new-world     # start fresh with the current definitions
cargo original      # built-in colors without installed Minecraft textures
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

Rust and Windows MSVC build tools are already available on this PC. The project
uses Rust 2024 and the checked-in Cargo.lock. Native rendering uses wgpu/Direct3D
12. Minecraft textures are read from the installed Windows game; they are not
stored in Git. Use `cargo original` without that installation.

To build and run the executable directly:

```powershell
cargo build --release --locked
.\target\release\mcbe-client.exe --engine --engine-home . --engine-continue
```

## Find the code

- `crates/mcbe-client/src/main.rs`: app state, window events and frame loop.
- `crates/mcbe-client/src/engine/`: playable engine, grouped by responsibility.
- `content/base/blocks/`: one JSON definition per block family.
- `content/base/models/` and `templates/`: shared models and component defaults.
- `content/base/recipes/crafting.json`: editable crafting recipes.
- `crates/mcbe-client/src/engine/blocks/`: typed components, state registry and compiled shapes.
- `crates/mcbe-core/`: reusable physics/inventory and earlier Bedrock data readers.
- `crates/mcbe-cli/`: optional research tools, separate from the playable entry point.

Start with [Architecture and source map](docs/ARCHITECTURE.md), then read
[Edit, run and commit](docs/DEVELOPMENT.md), [Blocks](docs/BLOCKS.md),
[Controls](docs/CONTROLS.md), [Chat and commands](docs/CHAT.md), [UI authoring](docs/UI.md), [Animations](docs/ANIMATIONS.md), [Inventory and storage](docs/INVENTORY.md), [Stations](docs/STATIONS.md), [Cameras and skins](docs/CAMERA.md), [Terrain](docs/TERRAIN.md), [Dimensions and generation](docs/DIMENSIONS.md), [Water and swimming](docs/FLUIDS.md), [Items and buckets](docs/ITEMS.md), [Movement](docs/MOVEMENT.md), [Rendering](docs/RENDERING.md) and [Milestone history](docs/HISTORY.md).

Walking, sprinting, sneaking, flight/noclip, inventory/crafting, JSON models,
slab/stair states, glass, unified component packs, optional signal behaviors,
crafting tables, persistent furnaces, F5 perspectives/free camera, personal skins,
first-person hands/held geometry, climate biomes/caves/ravines/oceans/rivers,
dynamic vertical chunks, conservative water flow/currents/swimming and measured buckets, settings, own PNG materials, background saves/loading, optional deferred sun/material lighting
and 2–255 chunk views are implemented. Nearby blocks retain full collision and
detail within 8 chunks. Distant hills/trees use stitched, progressively blended
geometry. Caves, transparent buildings and edits to distant generated trees are
not fully represented by that surface approximation. Maximum-range LOD uses
more memory than the earlier flat approximation.

Local vanilla cow, pig, creeper and zombie models/skins are optional resource
examples alongside the original creatures. Use `/spawn cow` (or another registered
kind) to try one on clear ground. Own PNG skins use the same cached instanced
renderer; see [Entities](docs/ENTITIES.md).

Run `cargo new-world` to try the three editable dimension examples. Press U or
Escape -> Dimensions to visit the surface, caverns and floating islands. Worlds
retain separate edits, fluids, furnaces, entities and return positions; inventory
and offhand are shared. Add named worlds through JSON; see DIMENSIONS.md.

Run `cargo new-world` to try the new terrain. Continuing an older save keeps its
original generator. Water flow, swimming and persistent measured buckets are implemented.

Runtime worlds, preferences, build output and verification captures are ignored
by Git. No launcher scripts or copied checkpoint packages belong in this project.

The former `Blockwork` project was renamed in place. Retained MCBE research is
under ignored `.local/legacy-research/`; there is no separate working project.

Ray-traced lighting is available through Escape -> Settings -> Rendering on
supported DX12 hardware. Forward remains the default for maximum speed.
Reflection/indirect switches and quality budgets are documented in
[Rendering](docs/RENDERING.md); unsupported hardware uses Deferred lighting.

Wall traversal: hold C to grip, W/S to climb, Ctrl plus movement to wall-run,
Space to jump away or pull onto a ledge, and Shift to drop.

Xbox/XInput controllers support gameplay, settings, inventory, station transfers
and search. Contextual bottom-corner hints switch with the active input device.
See [Controllers](docs/CONTROLLERS.md) for controls and UI glyph customization.

Data-authored mobs now wander, flee, follow and approach players, with shared
mouse/controller interaction, collision, instanced rendering and persistent saves.
See [Entities and mob AI](docs/ENTITIES.md) to register models and behaviors.

Offhand: equip the slot beside the inventory preview, V swaps hands, and Alt+RMB
uses the offhand (Xbox: RS click + X / LT). Both items render in F5 views.
Schema-12 saves preserve offhand contents and migrate older worlds.

Press T for local chat or / to start a command. `/give glass 64` adds items;
`/travel islands` visits a registered dimension. Tab completes names. See CHAT.md.
