# Goals

- Complex stroke styles, like photoshop or procreate.
- perfect AA (compute accurate subpixel coverage for strokes)
- varying stroke attributes
- textured strokes, with motion-coherent textures

# Next steps

Try pixel linked-list OIT. Draw strokes as splats in compute?

## Better coarse rasterization step

With conservative rasterization the curves appear more than once in each bucket (since it's per-triangle).
Plus the stroking width is a best guess.

## Occlusion culling

Difficult? Possibly based on an existing depth buffer. Or maybe big opaque strokes that are the equivalent of blocking.

## Assigning depth to stroke fragments

Derive fragment depth from depth of nearest curve point? There are options

## Simplification when too many transparent strokes overlap

???
Depends on the final rendering shader

# Textured strokes

## Foreshortening

TODO

## Final blending

The main advantage over HW rasterization is that it's more flexible. However, need to sort fragments by depth, low
occupancy.

Idea: per-tile, allocate just the right amount of memory for sorting ( proportional to the number of strokes )

Will need to look into OIT techniques anyway.
Shouldn't be splat-based, should eval lines/curves per pixel.

**Per-pixel list of: depth + curve index (depth can be omitted).**

## Alternate pipeline

1. Process every curve in parallel, "splat" directly the distance to curve & t value in the fragment buffer.
2. Sort & blend fragments

If "stroke interactions" aren't necessary:

1. Splat the integrated stroke directly in the fragment buffer, cull if alpha = 0 or fully occluded
2. Sort & blend

=> No HW rasterization, but the splatting shader is basically a custom rasterizer.

Alternative: stochastic transparency, removes the need for sorting, but no "stroke interactions"

## Stroke interactions / DF blending

In a nutshell: consider each stroke to be a DF, and allow the DF to be "distorted" by neighboring strokes.
For this, need to defer actual rendering of strokes as late as possible, until we know all stroke DFs interacting with a
pixel.

Basically, it's _non-local_ blending. Typical blending is done per-pixel, this is done at the stroke DF level.

**Unsure how "stroke interactions" are useful in practice**

## Shadows

lol

## Curve shapes

Taper, width profile.

## Ray tracing

# Questions

- shading
- stroke attributes
- stroke SDFs with subpixel accuracy

https://interplayoflight.wordpress.com/2022/06/25/order-independent-transparency-part-1/
https://www.reddit.com/r/GraphicsProgramming/comments/15l8bm9/order_independent_and_indiscriminate_transparency/

# Next steps

## OIT is costly

Fragment interlock tanks perfs. Many overlapping strokes also tanks perfs.
=> Fragment shader of initial rasterization must be ultra cheap!

- Additive blending? May be limited

## Try to repro specific effects

# UI/workflow woes

Too much boilerplate when adding new things:

- adding a pass requires modifying too many places (shaders struct, app ctor, reload_shaders, create_pipeline, render,
  plus much more if it requires resources)
- same with adding a parameter (app struct, ui(), push constants, shaders)
- worst: adding a selectable list of elements by name
    - e.g. brush textures, render mode

700 px
divide in 88 blocks of 8px

render at 88px, 1px = 8px

## Improvements

- creating pipelines: copy/pasting functions, update reload_pipelines, add field in App. Should be easier (PIPELINES)
    - remove the check for null pipeline options
- keeping struct and constants in sync between GLSL & Rust, and also shader interfaces (attachments, arguments) (
  INTERFACES***)
- resizing render targets as the window is resized (RESIZE)
    - to add a new render target, must modify three locations
- allocating and managing temporary render targets (TEMP)
- setting the viewport and scissors & related state (RENDERSTATE)
- allocating render targets with the correct usage (USAGE)
- to add a new UI option, need to change 3 locations (struct field, struct ctor, UI function) (UI)
- lists of options are cumbersome to implement in the UI (UI-LISTS)
- making sure that the format of images matches the shader interface; hard to experiment with because of the need to
  update multiple locations (FORMATS)
- samplers should really be defined next to where they are used, i.e. in shaders (SAMPLERS)
- more generally: adding stuff is just a lot of copy-paste, making the code unreadable; difficult to abstract because
  unclear about requirements of future algorithms
    - a wrong abstraction costs time if in the future it prevents an algorithm from being implemented efficiently
- reuse vertex or mesh+task shaders (REUSE)
- managing one-off image view objects is tedious (IMAGE-VIEWS)

General ideas: more hot-reloading, pipeline as data, GUIs, templates, and sane defaults

Sane defaults:

- viewport & scissors should take the size of the input texture by default

Templates:

- Build passes from templates

## Idea: UI for loading/saving global defines

* Add/remove/enable/disable global defines in the UI.
* On change, recompile all shaders.
* This is just `#define XXX`, no need to pass things in push constants.
* Good for quick tests.

-------------------------------------------------------

# Kinds of painting elements

- **Discrete elements**: leaves, blades of grass, individual strands of hair => something that "exists" (represents a
  concrete object) and is anchored in the world, not view-dependent
    - Goal: flexibility, lighting, shadowing
- **Shading elements**: lighting effects on hair, hair depiction, shadows on cloth, "fibrous" material appearance (like
  wood "figures") => material depiction
    - Goal: reproduce the appearance of overlapping semitransparent strokes
    - at first glance, hair depiction might seem like a "discrete element" problem, but strands of hair are rarely
      depicted individually. The strokes just give the "idea" of hair appearance.
        - it's not _always_ like that, hair depiction really blurs the line between discrete elements and materials
- **Contours**
- **Motion effects**

# Going forward

Taking things seriously:

- a separate application for painting might be too alienating, if the goal is for people to use it; safer to implement
  it as a blender plugin
    - render grease pencil primitives, but augmented with additional attributes, and animate them
    - the core of the application would still be a separate library, sharing its buffers with blender's opengl textures
    - see https://github.com/JiayinCao/SORT/ for an example for custom renderengine
    - also https://docs.blender.org/api/current/bpy.types.RenderEngine.html
- point of comparison: https://gakutada.gumroad.com/l/DeepPaint
- primary goal: get artists (not necessarily from studios) to use it and share their paintings on Twitter (or
  somewhere else)
    - **need to export animated results easily**
    - some people don't know how to animate => need **automatic animation** (turntable, move lights, etc.)
    - wow stuff: a painting that reacts to light changes, viewpoint changes
    - like live2d but "more"
- think about potential clients
- write project summary for submitting to incubators?
- ultimate goal: someone makes a music video with it

# Stroke engine

For actually rendering strokes. Two approaches:

- binned rasterization
- OIT / weighted OIT

Stroke ordering: keeping draw order is important

3D binning: bin curves in 2D + one "depth" or "ordering" dimension

## Idea: Coats

* Coats: group of strokes that have some unity in the painting process
* One render pass per coat / different coats are rendered in different passes.
* Simplified (weighted OIT) blending within a coat
* More complex blending possible between coats

Not all strokes have the same "footprint". Big vs fine details (of course, fine becomes big when zooming in).
How to evaluate the footprint? Depends on stroke width, curvature, curve length.

## Working around high curve counts per tile: depth coats

Assumption: high curve count per tile happens mostly because of camera viewpoints at grazing angles.
In this case: bin curves by screen-space depth. Process depth bins back-to-front.
Selection is done in task/mesh shader (don't split curves between depth bins).
Also, don't split user-defined coats.

1. (Task shader) coat LOD selection from object depth
2. (Mesh shader) emit geometry for curves, assign coat index
3. (Fragment shader) Binning: we have depth, coat index, position. Don't want to split same coat into different depth
   bins?

## Stroke engine parameters

* width procedural
* opacity procedural
* falloff (transverse opacity profile)
* stamp
* color procedural
* blending

## Degenerate strokes

Strokes that point toward or away from the camera. Stroke centerline mostly aligned with view direction.
Very small footprint on the screen because it's facing the camera.

In this case: remove the stroke.

In general: strokes make sense **if they have a significant curve-like footprint on the screen**. I.e. they have to
actually be
strokes, not points.

Observation: most strokes can be embedded into a 3D plane. Consider the normal of this 3D plane. If it's perpendicular
to the screen, then don't draw it (it's a degenerate stroke).
Issue: a lot of strokes are straight lines and are not embedded into only one plane.

## Mixed-order compositing

https://people.csail.mit.edu/ibaran/papers/2011-ASIA-MixedOrder.pdf

Paintings have max 30k strokes; let's target a round 100k strokes per frame. And say 32 subdivs per stroke, that's
3.2 million lines to store in the tiles (possibly more than once, given that lines may affect more than one tile).

At 1920x1080 with 16x16 tiles we have 8100 tiles; thus ~400 lines per tile assuming uniform stroke distribution.

## Transparency

Order:

- for "coats" on a level-set: try draw order. Should be valid outside grazing angles, but then the coat should fade away
  at those anyway.
    - fur, shading elements, "artifact" strokes
- otherwise, for discrete elements, use depth order

Blend modes:

- normal (alpha blending)
- screen
- overlay

Depth order: per-tile VS per-pixel
Generally, per-tile depth sorting isn't correct, high risk of visible discontinuities at tile boundaries

Within a tile, many lines will belong to the same curve. Pixels generated by lines belonging to the same curve will
overlap, and shouldn't blend together.