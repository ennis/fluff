
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
