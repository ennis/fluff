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

# V2 workflow

Load passes dynamically (from files).
Pass files contain

- shaders
- pipeline config
- rendering instructions

To start working on a pass, just copy/paste an existing file.

## Parameters:

Option A: automatically generated by reflecting the shader
Option B: described via a GUI and generate GLSL source code
Option C: described via a GUI, but must match the GLSL source code

Probably A, seems the most flexible.

Parameter types:

- Float, Vec2, etc.
- Parameters with specific names are automatically bound to the corresponding value (
  e.g. `u_time`, `u_frame`, `u_resolution`, `u_pixelSize`, etc.)

```rust

trait MeshShadingPass {
    fn source_file(&self) -> &str;
    fn buffers(&self) -> &[pass::BufferDesc];
}

const X = MeshShadingPass {
    source: SourceFile("..."),

};

```

## Shader bindings

## Geometry

How do we introduce new geometry formats without modifying the source?
We can't => fixed formats for:

- triangle meshes (surfaces)
- curves (beziers)
- lines
- polylines

A shader pass requests one type of geometry, under a certain form:

- vertex input
- storage buffers

## Images

Should be flexible on image formats => compile variants dynamically.
Access image formats in shaders via defines

## Shaders

Don't try to compose or fuse shaders together: one file == one pass (vertex->frag, or task->mesh->frag, or compute).


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

# Next steps

De-hardcode the rendering pipeline. Make it configurable.

- render pipeline config file containing:
    - resources to allocate (id, format, size, scale factor compared to the main render size)
    - resources to load from a file
        - textures
        - texture sets (loaded in a descriptor array)
            - load from file pattern (e.g. everything in a directory, or image sequences)
    - render passes (type, shaders)
      -> main idea: user assigns IDs to resources/resource sets in the config file, and they become automatically
      accessible in shaders
- shaders can refer to resources in shaders via their IDs like so:
  `layout(texid_position) uniform texture2D position;` => `texid_position` is a macro that expands to `set=0, binding=n`
  or similar, automatically generated by the engine
- resource usage is inferred from usage in shaders (by reflecting the uniforms)
- geometry is still loaded from the app
- rendering engine should be embeddable

- `engine` common types, shaders, resource ids, passes
    - `config` parser for the config file
    - `shader` shader reflection (detect usages, etc.)
    - `runtime` actually allocating and loading resources, building the command buffers,

# Generic geometry

- curves
- attributes

# Uniform parameters

Currently: add a UI entry, add a field to the App struct, add a field to the PushConstants struct, add field in block in
shader
=> 4 locations to change
Reduce that to two:

- declare parameter in engine
- add field in shader

Complications:

- Difficult to bind parameters in engine to what the shader expects ()

# Simpler resource bindings

All shaders get the same set of descriptors. It is declared in a common header (e.g. "resources.glsl").
Bindings are assigned sequentially by the compiler. No SPIR-V remapping necessary because the binding numbers are the
same for all shaders. The shader can assign explicit binding numbers, but that shouldn't be necessary.

Must be careful that all shaders have the same descriptors though.

Step 1: compile all shaders, collect bindings, check for conflicts
Step 2: create universal descriptor set layout
Step 3: create pipelines
Step 4: allocate resources
Step 5: build universal descriptor set or descriptor buffer

Issue: can't escape descriptor set layouts on vulkan, even though they don't exist in metal (they would be a no-op).

# Even simpler resource bindings

Instead of trying to reflect the descriptor set layout from the shaders, use a "universal" layout that basically
emulates fixed bindings.

In graal: specify maximum number of descriptors of each type during pipeline creation, instead of a precise layout.
Then create argument buffers, which use the same limit (and internally use the same descriptor set layout).

Shape of these layouts:

- Set layout #0
    - 32 uniform buffers
    - 32 storage buffers
    - 32 textures
    - 32 storage images
    - 32 samplers
- Set layout #1
    - unbounded uniform buffer array
- Set layout #2
    - unbounded storage buffer array
- Set layout #3
    - unbounded texture array
- Set layout #4
    - unbounded storage image array

Or:

- Set 0:
    - 16 uniform buffers
- Set 1:
    - 32 storage buffers
    - indexed storage buffer array
- Set 2:
    - 32 textures
    - 16 samplers
    - unbounded texture array
- Set 3:
    - 32 images
    - unbounded image array

## Universal argument buffer

Two strategies:

- push descriptors for "light-weight", one-off stuff
- universal argument buffer for the rest

Universal argument buffers are sets of descriptor sets that contain all bindings for a pipeline.
The intended usage is to stuff every possible resource in them regardless of whether they are used in shaders,
bind the descriptor sets at the beginning of the frame, and forget about it.

A nice thing about that is that the layout of the universal descriptor sets are standardized, so there's no need to
adapt them to the shaders.

# Resource forwarding

Sometimes it's necessary to skip some parts of the pipeline depending on a toggle in the UI. But we don't want to
recompile/reallocate everything.

Solutions:

1. enable/disable flag on passes + resource aliasing

Allow "aliasing" resources to others, in a way that doesn't invalidate the pipelines & the resources. Tricky because
resource usages must be compatible after aliasing.
(e.g. when aliasing an image to another, it may suddenly need to have the COLOR_TARGET usage because of how the
destination is used; and thus it needs to be reallocated).

=> Infer usages of concrete resources later

2. Late-bound resources + manually execute each pass
   Difficult, would need to update the descriptors on-the-fly. Incompatible with our bindless approach.

3. enable/disable flag + additional indirection
   When defining passes, use "virtual resources". Each virtual resource has its own binding number.

4. don't forward, just blit instead

5. Forwarding is actually a special kind of pass that can be enabled or disabled
   => No, disabling passes shouldn't invalidate the bindings

6. Don't alias resources, just recompile everything should the pass structure change.

# Frontend/backend separation

Frontend: manage logical/physical resources, infer usages

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

