
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
For this, need to defer actual rendering of strokes as late as possible, until we know all stroke DFs interacting with a pixel.

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
- adding a pass requires modifying too many places (shaders struct, app ctor, reload_shaders, create_pipeline, render, plus much more if it requires resources)
- same with adding a parameter (app struct, ui(), push constants, shaders)
- worst: adding a selectable list of elements by name
  - e.g. brush textures, render mode


700 px
divide in 88 blocks of 8px

render at 88px, 1px = 8px 


## Improvements
- creating pipelines: copy/pasting functions, update reload_pipelines, add field in App. Should be easier (PIPELINES)
- keeping struct and constants in sync between GLSL & Rust, and also shader interfaces (attachments, arguments) (INTERFACES***)
- resizing render targets as the window is resized (RESIZE)
- allocating and managing temporary render targets (TEMP)
- setting the viewport and scissors & related state (RENDERSTATE)
- to add a new UI option, need to change 3 locations (struct field, struct ctor, UI function) (UI) 
- lists of options are cumbersome to implement in the UI (UI-LISTS)
- making sure that the format of images matches the shader interface; hard to experiment with because of the need to update multiple locations (FORMATS)
- samplers should really be defined next to where they are used, i.e. in shaders (SAMPLERS)
- more generally: adding stuff is just a lot of copy-paste, making the code unreadable; difficult to abstract because unclear about requirements of future algorithms
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
- Parameters with specific names are automatically bound to the corresponding value (e.g. `u_time`, `u_frame`, `u_resolution`, `u_pixelSize`, etc.)

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

- **Discrete elements**: leaves, blades of grass, individual strands of hair => something that "exists" (represents a concrete object) and is anchored in the world, not view-dependent 
    - Goal: flexibility, lighting, shadowing
- **Shading elements**: lighting effects on hair, hair depiction, shadows on cloth, "fibrous" material appearance (like wood "figures") => material depiction
    - Goal: reproduce the appearance of overlapping semitransparent strokes
    - at first glance, hair depiction might seem like a "discrete element" problem, but strands of hair are rarely depicted individually. The strokes just give the "idea" of hair appearance.
        - it's not _always_ like that, hair depiction really blurs the line between discrete elements and materials
- **Contours**
- **Motion effects**
