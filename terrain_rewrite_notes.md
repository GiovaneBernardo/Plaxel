# Terrain Rewrite Notes

This file captures the important conclusions from the terrain discussion so it can be picked up later in a normal chat.

## Main conclusion

For this game, the safest long-term direction is a **sparse voxel octree with fixed-size brick leaves**.

Not:
- pure octree leaves as direct mesh chunks
- pure sparse bricks with no hierarchy
- sparse voxel DAG for live editable terrain

The practical split is:
- **octree / hierarchy** decides what space matters and what LOD is needed
- **brick leaves** store and mesh terrain data in a regular fixed format

## What "brick leaves" means

An octree leaf should not be the mesh itself.
It should point to a regular chunk of terrain data.

Example:

```text
octree leaf -> BrickCoord(level, x, y, z)
brick -> fixed sample grid -> dual contouring
```

The leaf is the address of the terrain, not the terrain shape.

## Important takeaway from the failed sparse-brick attempt

The sparse-brick-only attempt got slow because:

- the selection was too broad
- the system still tried to cover too much of the planet at once
- far chunks used too much SDF sampling
- the active-set logic still behaved like a big volume flood
- debug rendering also inflated the apparent cost

So the lesson is not "bricks are bad".
The lesson is that **bricks need a hierarchy and tight selection**.

## Recommended data model

Use:

```rust
struct BrickCoord {
    level: u8,
    x: i32,
    y: i32,
    z: i32,
}

struct Brick {
    coord: BrickCoord,
    sdf: Vec<f32>,
    material: Vec<u8>,
    dirty: bool,
}
```

Suggested rules:
- fixed cell count per brick, usually `32^3`
- discrete LOD levels only
- same meshing path for every brick
- only world scale changes with level

## What the octree should do

The octree should answer:
- does this region possibly contain surface?
- should it subdivide?
- which brick LOD should be active here?

The octree should not be the thing that directly defines arbitrary mesh sizes.

## What the brick should do

The brick should:
- store the fixed-format voxel/SDF data
- generate the mesh
- store edit deltas
- be easy to serialize
- be easy to rebuild when dirty

## Why DAG is not a good default here

Sparse voxel DAGs are great for compressing static terrain.

They are not a great default for live deformable terrain because:
- edits break subtree sharing
- rebuilds get complicated
- the data structure becomes brittle under frequent changes

If used at all, DAGs should be reserved for immutable base terrain, not the live edited layer.

## Steps for a clean rewrite

1. Define the terrain contract.
   - world-space SDF
   - optional material field
   - optional edit delta field

2. Define node states.
   - `Empty`
   - `Solid`
   - `Mixed`

3. Make the leaf-to-brick mapping explicit.
   - leaf carries `BrickCoord`
   - brick carries actual samples

4. Keep LOD discrete.
   - level 0, 1, 2, 3, etc.
   - each level is a predictable scale step

5. Separate classification from meshing.
   - classification decides if a region matters
   - meshing builds vertices

6. Add ghost / padding samples.
   - needed for stable boundary behavior
   - important for dual contouring continuity

7. Add a priority queue.
   - visible first
   - close first
   - physics next
   - background preload last

8. Keep the upload budget bounded.
   - no unbounded worker flood
   - no unbounded mesh uploads per frame

9. Dirty only what changes.
   - edited brick
   - adjacent boundary bricks
   - affected subtree

10. Add physics later.
   - close bricks only
   - tight budget
   - optional toggle

## What to avoid

- Don't make the whole planet active as a volumetric brick flood.
- Don't mesh arbitrary octree leaf sizes directly.
- Don't rely on a sample-heavy selector for far terrain.
- Don't use one huge debug volume pass while tuning performance.

## Noise / terrain generation notes

Important lesson:
- the SDF cost was too high mainly because too many samples were evaluated
- far chunks should use fewer samples than close chunks
- the terrain function itself should stay readable and directional, not flat-world Y-up

The noise function should be:
- spherical / radial
- lower frequency at large scale
- only moderately warped
- cheaper on coarse bricks

## Practical recommendation

If restarting from scratch:
- use sparse octree for hierarchy and selection
- use fixed-size brick leaves for terrain data and meshing
- keep dual contouring
- add proper cross-LOD boundary handling in the extraction path
- avoid DAGs for mutable terrain

## Current status of the earlier prototype

The earlier sparse-brick prototype showed that:
- coverage-first selection is necessary
- far-level sampling must be much cheaper
- debug drawing can distort performance perception
- whole-planet sparse-brick-only rendering is not a good default
