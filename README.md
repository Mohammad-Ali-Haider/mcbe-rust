# Minecraft Rust Edition

An expandable Rust voxel engine for your own block game. This is the single
active project; development versions now live in Git history.

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
- `assets/blocks.json`: registration, shapes, materials and collision.
- `assets/recipes.json`: editable crafting recipes.
- `crates/mcbe-core/`: reusable physics/inventory and earlier Bedrock data readers.
- `crates/mcbe-cli/`: optional research tools, separate from the playable entry point.

Start with [Architecture and source map](docs/ARCHITECTURE.md), then read
[Edit, run and commit](docs/DEVELOPMENT.md), [Blocks](docs/BLOCKS.md),
[Controls](docs/CONTROLS.md) and [Milestone history](docs/HISTORY.md).

Walking, sprinting, sneaking, flight/noclip, inventory/crafting, JSON models,
slab/stair states, glass, own PNG materials, settings, background saves/loading
and 2–255 chunk views are implemented. Nearby blocks retain full collision and
detail within 8 chunks. Distant hills/trees use stitched, progressively blended
geometry. Caves, transparent buildings and edits to distant generated trees are
not fully represented by that surface approximation. Maximum-range LOD uses
more memory than the earlier flat approximation.

Runtime worlds, preferences, build output and verification captures are ignored
by Git. No launcher scripts or copied checkpoint packages belong in this project.

The former `Blockwork` project was renamed in place. Retained MCBE research is
under ignored `.local/legacy-research/`; there is no separate working project.
