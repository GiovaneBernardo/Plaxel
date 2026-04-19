//! Thin shim: being a `dylib` that depends on `engine` forces `engine`'s
//! object code into a single shared library, so the game exe and the
//! hot-reloaded `game_logic.dll` resolve engine symbols (and `TypeId`s)
//! through the same instance.
//!
//! No re-export needed — downstream crates still `use engine::...` directly;
//! rustc routes those symbols through this dylib at link time.
