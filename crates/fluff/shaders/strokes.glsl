#version 460 core
#include "bindless.inc.glsl"
#include "shared.inc.glsl"
#include "common.inc.glsl"

#extension GL_EXT_mesh_shader : require
#extension GL_ARB_fragment_shader_interlock : require
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_KHR_shader_subgroup_ballot : require
#extension GL_KHR_shader_subgroup_arithmetic : require
#extension GL_KHR_shader_subgroup_shuffle : require
#extension GL_EXT_debug_printf : enable
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

//const uint POLYLINE_START = 1;
//const uint POLYLINE_END = 2;
//const uint POLYLINE_SCREEN_SPACE = 4;

//const uint MAX_POLYLINE_VERTICES = 16384;
//const uint MESH_WORKGROUP_SIZE = 32;

//////////////////////////////////////////////////////////

// Represents a stroke to be expanded to triangles by a mesh shader workgroup.
// A stroke has a maximum of vtxCount=512 vertices (for 511 segments).
//
// Mesh workgroups can process multiple strokes at once. For instance, with SUBGROUP_SIZE == 32,
// a workgroup can process:
//
// - 16 strokes with vtxCount=2
// - 8 strokes with vtxCount=4
// - 4 strokes with vtxCount=8
// - 2 strokes with vtxCount=16
// - 1 stroke with vtxCount=32
//
// Note that the strokes are binned in the task shader so that across a mesh workgroup all strokes have the same
// segment count rounded up to a power of two.
//
// For strokes with more than SUBGROUP_SIZE vertices, multiple workgroups are launched, each workgroup will process a fraction
// of the stroke:
//
// - 1/2 stroke per workgroup with vtxCount=64
// - 1/4 stroke per workgroup with vtxCount=128
// - 1/8 stroke per workgroup with vtxCount=256
// - 1/16 stroke per workgroup with vtxCount=512
//
// The maximum number of workgroups is reached when all strokes in the task shader workgroup have vtxCount=MAX_VTX_COUNT=512,
// i.e. maxWorkgroupCount =
//              (task shader wgroup size) * MAX_VTX_COUNT / SUBGROUP_SIZE
//            = SUBGROUP_SIZE * MAX_VTX_COUNT / SUBGROUP_SIZE
//            = MAX_VTX_COUNT
struct PackedStroke {
    uint8_t strokeIdx;     // index relative to the baseStrokeID: strokeID = baseStrokeID + strokeIdx
    uint8_t log2VertexCount;     // log2 number of vertices in the stroke: log2(2,4,8,16,32,64,256,512) = (1,2,3,4,5,6,8,9)
    uint16_t wgroupMatch;  // if the entry is the first of a bin, this is the first workgroup ID of the bin, otherwise it's the first workgroup ID of the next bin
};


// Shared task payload.
struct TaskData {
    uint baseStrokeID;
    PackedStroke strokes[SUBGROUP_SIZE];
};

layout(scalar, push_constant) uniform PushConstants {
    DrawStrokesPushConstants u;
};

//////////////////////////////////////////////////////////

#ifdef __TASK__

layout(local_size_x=SUBGROUP_SIZE) in;

taskPayloadSharedEXT TaskData taskData;


// Inspired by https://github.com/nvpro-samples/vk_displacement_micromaps/blob/main/micromesh_binpack.glsl
// Returns the total number of workgroups to launch.
uint binPack(uint strokeVertexCount) {
    uint laneID = gl_SubgroupInvocationID;

    // For strokes that can't be processed in a single workgroup, we need additional vertices to make the junction between meshlets.
    //
    // E.g. if we do things naively, a stroke with 64 vertices and subgroupSize=32 will be expanded in two meshlets:
    // - meshlet 1 with vertices 0-31 and 31 quads
    // - meshlet 2 with vertices 32-63 and 31 quads
    // With this scheme, the quad between vertices 31 and 32 is missing. We can't add it easily because its
    // vertices end up in different meshlets.
    //
    // Our solution is to introduce a one-vertex overlap between meshlets:
    // - meshlet 1 with vertices 0-31 and 31 quads
    // - meshlet 2 with vertices 31*-62 and 31 quads (note that vertex 31 is in both meshlets)
    // - meshlet 3 with vertices 62*-63 and 1 quad
    // You'll notice that meshlet 3 wastes a lot of occupancy. I don't have an answer to that ¯\_(ツ)_/¯
    uint overlaps = strokeVertexCount / SUBGROUP_SIZE;
    uint vertexCount = roundUpPow2(strokeVertexCount + overlaps);

    uvec4 vote;
    // This can be replaced with a loop or ideally subgroupPartitionNV if available
    if (vertexCount == 2) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 4) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 8) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 16) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 32) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 64) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 128) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 256) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 512) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 1024) {
        vote = subgroupBallot(true);
    } else if (vertexCount == 2048) {
        vote = subgroupBallot(true);
    } else {
        vote = uvec4(0);
    }

    uint binFirst = subgroupBallotFindLSB(vote);
    // size of the bin = number of strokes with the same vtx count
    uint binSize = subgroupBallotBitCount(vote);
    // index of this stroke in the bin
    uint binPrefix = subgroupBallotExclusiveBitCount(vote);

    bool isBinFirst = binFirst == laneID;
    // sum of the sizes of all bins before this one
    uint binStartIdx = subgroupExclusiveAdd(isBinFirst ? binSize : 0);
    binStartIdx = subgroupShuffle(binStartIdx, binFirst);

    uint outID = binStartIdx + binPrefix;

    // Determine number of workgroups for the bin (or equivalently the number of meshlets
    // since each workgroup produces one meshlet).

    uint wgroupCount = divCeil(binSize * vertexCount, SUBGROUP_SIZE);

    // First workgroup of the bin
    uint wgroupMatch = subgroupExclusiveAdd(isBinFirst ? wgroupCount : 0);
    wgroupMatch = subgroupShuffle(wgroupMatch, binFirst);
    // Subtlety: for the first entry in the bin, store the actual first workgroup for the bin,
    // but for others, store the first workgroup of the **next bin**, so that in the mesh shader we can
    // retrieve the first of the bin with `subgroupBallot(wgroupID >= wgroupMatch) / subgroupFindMSB()`.
    wgroupMatch = isBinFirst ? wgroupMatch : wgroupMatch + wgroupCount;

    taskData.strokes[outID] = PackedStroke(uint8_t(laneID), uint8_t(findMSB(vertexCount)), uint16_t(wgroupMatch));
    uint wgroupTotalCount = subgroupAdd(isBinFirst ? wgroupCount : 0);
    return wgroupTotalCount;
}

void main() {

    // initialize task data
    taskData.strokes[gl_SubgroupInvocationID] = PackedStroke(uint8_t(0), uint8_t(0), uint16_t(0xFFFF));
    subgroupBarrier();

    // Process 32 (SUBGROUP_SIZE) strokes at once,
    uint strokeID = gl_GlobalInvocationID.x;
    if (strokeID < u.strokeCount) {
        uint wgroupCount = binPack(u.strokes.d[strokeID].vertexCount);
        if (gl_LocalInvocationIndex == 0) {
            taskData.baseStrokeID = strokeID;
            EmitMeshTasksEXT(wgroupCount, 1, 1);
        }
    }
}

#endif

//////////////////////////////////////////////////////////

#ifdef __MESH__

vec4 project(vec3 pos)
{
    vec4 p = u.sceneParams.d.viewProj * vec4(pos, 1.0);
    p.y = -p.y;
    return p;
}

taskPayloadSharedEXT TaskData taskData;

layout(location=0) out vec2 o_position[];
layout(location=1) out vec4 o_color[];
layout(location=2) out float o_width[];
layout(location=3) out float o_arcLength[];
layout(location=4) out perprimitiveEXT flat int o_brush[];
layout(location=5) out perprimitiveEXT flat int o_strokeID[];
layout(location=6) out flat vec2 o_normal[];
layout(location=7) out vec2 o_pixelPosition[];
layout(location=8) out perprimitiveEXT flat vec2 o_radii[];
layout(location=9) out perprimitiveEXT flat float o_startArcLength[];
layout(location=10) out perprimitiveEXT flat float o_segmentLength[];

layout(local_size_x=SUBGROUP_SIZE) in;

// Process groups of 32 (SUBGROUP_SIZE) vertices at once, which expand to 64 vertices forming 62 triangles.
layout(triangles, max_vertices = SUBGROUP_SIZE*2, max_primitives = (SUBGROUP_SIZE-1)*2) out;


struct WorkgroupConfig {
    // the stroke to process
    uint strokeID;
    // which vertex of the stroke
    uint vertexIdx;
    // number of vertices in the stroke rounded up to a power of 2 (same across invocations in the wg)
    uint vertexCountPow2;
    // actual number of vertices in the stroke
    uint vertexCount;
    // which part of the stroke, if the process is split across multiple wgs
    uint partIdx;
    // vertex index in the output meshlet
    uint meshletVtxOffset;
    // number of vertices in the output meshlet
    uint meshletVtxCount;
    // prim index in the output meshlet
    uint meshletPrimOffset;
    // number of primitives in the output meshlet
    uint meshletPrimCount;
    bool valid;
    //bool cull;
};

WorkgroupConfig binUnpack(uint wgroupID)
{
    WorkgroupConfig cfg;
    cfg.valid = false;

    uint laneID = gl_SubgroupInvocationID;
    uint wgroupMatch = taskData.strokes[laneID].wgroupMatch;
    uvec4 vote = subgroupBallot(wgroupID >= wgroupMatch);
    if (vote == uvec4(0)) {
        return cfg;
    }
    uint binStart = subgroupBallotFindMSB(vote);

    uint log2VertexCount = taskData.strokes[binStart].log2VertexCount;
    cfg.vertexCountPow2 = 1 << log2VertexCount;     // number of threads per packed curve
    uint wgroupStart = taskData.strokes[binStart].wgroupMatch;   // first workgroup of the bin
    uint wgroupOffset = wgroupID - wgroupStart;   // offset of this workgroup in the bin

    uint binThreadID = wgroupOffset * SUBGROUP_SIZE + laneID;
    //uint binThreadCount = cfg.vertexCountPow2 * SUBGROUP_SIZE;
    uint binStrokeIdx = binThreadID / cfg.vertexCountPow2;
    uint inID = binStart + binStrokeIdx;

    uint overlaps = cfg.vertexCountPow2 > SUBGROUP_SIZE ? (binThreadID % cfg.vertexCountPow2) / SUBGROUP_SIZE : 0;
    cfg.vertexIdx = binThreadID % cfg.vertexCountPow2 - overlaps;
    cfg.partIdx = cfg.vertexIdx / SUBGROUP_SIZE;

    // check for overflow and that we're not reading the next bin
    cfg.valid = inID < SUBGROUP_SIZE && taskData.strokes[inID].log2VertexCount == log2VertexCount;

    if (cfg.valid) {
        cfg.strokeID = taskData.baseStrokeID + taskData.strokes[inID].strokeIdx;
        cfg.vertexCount = u.strokes.d[cfg.strokeID].vertexCount;

        // check that we're not going beyond the stroke vertex count
        // this can happen if the stroke vertex count is not a power of 2
        if (cfg.vertexIdx >= cfg.vertexCount) {
            // clamp idx to the last vertex, and cull the corresponding quad
            // we don't want to set cfg.valid to false, as we still need to output data in the meshlet
            cfg.vertexIdx = cfg.vertexCount - 1;
        }

        const uint maxVertsPerMeshlet = SUBGROUP_SIZE * 2;
        cfg.meshletVtxOffset =  (2 * binThreadID) % maxVertsPerMeshlet;
        cfg.meshletVtxCount = subgroupMax(cfg.meshletVtxOffset) + 2;
        cfg.meshletPrimOffset = cfg.meshletVtxOffset;
        cfg.meshletPrimCount = cfg.meshletVtxCount - 2;
    }

    return cfg;
}

void main()
{
    uint laneID = gl_SubgroupInvocationID;  // It's also gl_LocalInvocationIndex
    WorkgroupConfig cfg = binUnpack(gl_WorkGroupID.x);

    if (cfg.valid) {
        bool isFirst = cfg.vertexIdx == 0;
        bool isLast = cfg.vertexIdx == cfg.vertexCount - 1;

        Stroke stroke = u.strokes.d[cfg.strokeID];
        uint vertex = stroke.baseVertex + cfg.vertexIdx;

        StrokeVertex v = u.vertices.d[vertex];
        StrokeVertex v0 = isFirst ? v : u.vertices.d[vertex - 1];
        StrokeVertex v1 = isLast ? v : u.vertices.d[vertex + 1];

        #ifdef WIDTH_NOISE
        v.width =  uint8_t(clamp(v.width / 255.0 + 0.1 * noise(vec2(v.s, 0.)), 0., 1.) * 255.);
        v0.width = uint8_t(clamp(v.width / 255.0 + 0.1 * noise(vec2(v0.s, 0.)), 0., 1.) * 255.);
        v1.width = uint8_t(clamp(v.width / 255.0 + 0.1 * noise(vec2(v1.s, 0.)), 0., 1.) * 255.);
        #endif


        #ifdef WAVY_STROKES
        float wave = 0.002 * sin(2000.0 * u.sceneParams.d.time + cfg.strokeID * 1000.0);
        v.pos.x += wave;
        v0.pos.x += wave;
        v1.pos.x += wave;
        #endif

        vec4 p = project(v.pos);
        vec4 p0 = project(v0.pos);
        vec4 p1 = project(v1.pos);

        // half-width + anti-aliasing margin
        // clamp the width to 1.0, below that we just fade out the line
        float width = u.width;
        #ifdef TIGHT_GEOMETRY
        width *= v.width / 255.0;
        #endif
        float hwAA = max(width, 1.0) * 0.5 + u.filterWidth * sqrt(2.0);
        vec2 pxSize = vec2(2) / u.sceneParams.d.viewportSize; // pixel size in clip space
        //float width = v.width / 255.0;

        // compute normals
        vec2 n;
        vec2 t;
        if (isFirst) {
            vec2 v = p1.xy/p1.w - p.xy/p.w;
            n = hwAA * pxSize * normalize(pxSize * vec2(-v.y, v.x));
            t = -hwAA * normalize(v/ pxSize) * pxSize;
        } else if (isLast) {
            vec2 v = p.xy/p.w - p0.xy/p0.w;
            n = hwAA * pxSize * normalize(pxSize * vec2(-v.y, v.x));
            t = hwAA * normalize(v/ pxSize) * pxSize;
        }
        else {
            vec2 v0 = normalize((p.xy/p.w - p0.xy/p0.w) / pxSize);
            vec2 v1 = normalize((p1.xy/p1.w - p.xy/p.w) / pxSize);
            vec2 vt = 0.5 * (v0 + v1);
            n = vec2(-vt.y, vt.x);
            // half-width / sin(theta/2)
            float d = hwAA / max(cross(vec3(v0, 0.0), vec3(n, 0.0)).z, 1.0);
            n = d * n * pxSize;
            t = vec2(0.);
        }

        vec4 a = p;
        vec4 b = p;
        a.xy += (-n + t) * a.w;
        b.xy += (n + t) * b.w;



        uint voff = cfg.meshletVtxOffset;
        gl_MeshVerticesEXT[voff].gl_Position = a;
        gl_MeshVerticesEXT[voff+1].gl_Position = b;
        o_color[voff] = vec4(v.color) / 255.0;
        o_color[voff+1] = vec4(v.color) / 255.0;
        o_color[voff].a *= v.opacity / 255.0;
        o_color[voff+1].a *= v.opacity / 255.0;
        o_width[voff] = (isFirst || isLast) ? 0.0 : (v.width / 255.0);
        o_width[voff+1] = (isFirst || isLast) ? 0.0 : (v.width / 255.0);
        o_arcLength[voff] = v.s;
        o_arcLength[voff+1] = v.s;

        vec2 ssNormal = normalize(n / pxSize);
        ssNormal.y = -ssNormal.y;
        o_normal[voff] = ssNormal;
        o_normal[voff+1] = ssNormal;
        #ifdef HIGHLIGHT_LONG_STROKES
        // < 32: green
        // 32-64: yellow
        // 64-128: orange
        // 128-256: red
        // 256-512: magenta
        // > 512: white
        vec4 color;
        if (cfg.vertexCount < 4) {
            color = vec4(0.0, 0.5, 1.0, 1.0);
        }
        else if (cfg.vertexCount < 8) {
            color = vec4(0.0, 0.8, 0.8, 1.0);
        }
        else if (cfg.vertexCount < 16) {
            color = vec4(0.0, 1.0, 0.3, 1.0);
        }
        else if (cfg.vertexCount < 32) {
            color = vec4(0.0, 1.0, 0.0, 1.0);
        }
        else if (cfg.vertexCount < 2*32) {
            color = vec4(1.0, 1.0, 0.0, 1.0);
        }
        else if (cfg.vertexCount < 4*32) {
            color = vec4(1.0, 0.3, 0.0, 1.0);
        }
        else if (cfg.vertexCount < 8*32) {
            color = vec4(1.0, 0.0, 0.0, 1.0);
        }
        else if (cfg.vertexCount < 16*32) {
            color = vec4(1.0, 0.0, 1.0, 1.0);
        }
        else {
            color = vec4(1.0, 1.0, 1.0, 1.0);
        }
        o_color[voff+0] = color;
        o_color[voff+1] = color;
        #endif
        float x = isFirst || isLast ? -hwAA : 1.0;
        o_position[voff+0] = vec2(x, -hwAA);
        o_position[voff+1] = vec2(x, hwAA);
        o_pixelPosition[voff] = vec2(v.s, -hwAA);
        o_pixelPosition[voff+1] = vec2(v.s, hwAA);

        if (isLast) {
            debugPrintfEXT("strokeID=%4d, vertexIdx=%4d, vertexCount=%4d, meshletVtxCount=%4d, meshletPrimCount=%4d, meshletVtxOffset=%4d\n", cfg.strokeID, cfg.vertexIdx, cfg.vertexCount, cfg.meshletVtxCount, cfg.meshletPrimCount, cfg.meshletVtxOffset);
        }

        if (cfg.meshletPrimOffset < cfg.meshletPrimCount - 1) {
            uint poff = cfg.meshletPrimOffset;
            gl_PrimitiveTriangleIndicesEXT[poff] = uvec3(voff, voff+2, voff+1);
            gl_PrimitiveTriangleIndicesEXT[poff+1] = uvec3(voff+2, voff+3, voff+1);
            gl_MeshPrimitivesEXT[poff].gl_CullPrimitiveEXT = isLast;
            gl_MeshPrimitivesEXT[poff+1].gl_CullPrimitiveEXT = isLast;
            o_brush[poff] = stroke.brush;
            o_brush[poff+1] = stroke.brush;
            o_strokeID[poff] = int(cfg.strokeID);
            o_strokeID[poff+1] = int(cfg.strokeID);
            o_radii[poff] = vec2(v.width / 255., v1.width / 255.);
            o_radii[poff+1] = vec2(v.width / 255., v1.width / 255.);
            o_startArcLength[poff] = v.s;
            o_startArcLength[poff+1] = v.s;
            o_segmentLength[poff] = v1.s - v.s;
            o_segmentLength[poff+1] =  v1.s - v.s;
        }

        if (gl_LocalInvocationIndex == 0) {
            SetMeshOutputsEXT(cfg.meshletVtxCount, cfg.meshletPrimCount);
        }
    }
}

#endif

//////////////////////////////////////////////////////////

#ifdef __FRAGMENT__

layout(location=0) in vec2 i_position;
layout(location=1) in vec4 i_color;
layout(location=2) in float i_width;
layout(location=3) in float i_arcLength;
layout(location=4) in perprimitiveEXT flat int i_brush;
layout(location=5) in perprimitiveEXT flat int i_strokeID;
layout(location=6) in vec2 i_normal;
layout(location=7) in vec2 i_pixelPosition;
layout(location=8) in perprimitiveEXT flat vec2 i_radii;
layout(location=9) in perprimitiveEXT flat float i_startArcLength;
layout(location=10) in perprimitiveEXT flat float i_segmentLength;
layout(location=0) out vec4 o_color;

/*
// 2D Distance from a point to a segment.
float distSeg(vec2 p, vec2 a, vec2 b, out float alpha) {
    vec2 ab = b - a;
    vec2 ap = p - a;
    float side = sign(cross(vec3(ab, 0.0), vec3(ap, 0.0)).z);
    float d = dot(p - a, ab) / dot(ab, ab);
    d = clamp(d, 0.0, 1.0);
    vec2 p0 = a + d * ab;
    alpha = d;
    //float taper = max(0.0, 80.0 - distance(p,b)) / 80.0;
    return side * distance(p, p0);
}*/


float intEllipse(vec2 ro, vec2 rd, vec2 ab)
{
    vec2 ocn = ro / ab;
    vec2 rdn = rd / ab;
    float a = dot( rdn, rdn );
    float b = dot( ocn, rdn );
    float c = dot( ocn, ocn );
    float h = b*b - a*(c-1.0);
    return h < 0. ? 0. : abs(2 * sqrt(h) / a);
}

void main() {
    // clamped width
    float width = u.width;

    #ifdef BRUSH_PRESSURE_TEST
    width *= i_width;
    #endif

    Stroke stroke = u.strokes.d[i_strokeID];

    float width1 = max(width, 1.0);
    float h = width1 * 0.5;
    float _a;
    float y = distSeg(i_position, vec2(0.0, 0.0), vec2(1.0, 0.0), _a);
    #ifdef NO_ENDCAPS
    y = i_position.y;
    #endif
    //float filterWidth = 1.5;
    float halfFilterWidth = u.filterWidth * 0.5;
    float alpha = (clamp((abs(y) + h + halfFilterWidth), 0., width1) - clamp((abs(y) + h - halfFilterWidth), 0., width1)) / u.filterWidth;
    alpha *= min(width, 1.0);

    #ifdef SMOOTH_BRUSH_TEST
    alpha *= 1.0 - smoothstep(0.0, 1.0, abs(y)/h);
    #endif

    #ifdef AIRBRUSH_TEST
    // https://shenciao.github.io/brush-rendering-tutorial/Basics/Stamp/
    float r1 = h;//.5 * u.width * i_radii.x; //.5 * u.width;
    float r0 = h;//.5 * u.width * i_radii.y; //.5 * u.width;
    float D = (r1 - r0) / i_segmentLength;
    //float y = i_pixelPosition.y;
    float xp = (i_pixelPosition.x - i_startArcLength);
    float a = D*D - 1.;
    float b = 2*(r0*D-xp);
    float c = r0*r0 - xp*xp - y*y;
    float disc = b*b - 4.0*a*c;
    if(disc <= 0.0) discard;
    float t0 = (-b - sqrt(disc)) / (2.*a);
    float t1 = (-b + sqrt(disc)) / (2.*a);
    float lr = abs(t1 - t0);
    const float alpha_density = 0.005;
    alpha = 1. - exp(-alpha_density * lr);

    #ifdef STAMPED
    uint brushIndex = i_brush;
    #ifdef TEXTURE_OVERRIDE
    brushIndex = u.brush;
    #endif
    image2DHandle tex = u.brushTextures.d[brushIndex];
    alpha = 0.;
    const float texSize = 256;
    const float stampSpacing = 1.0;
    int t0r = int(ceil((i_startArcLength + t0) / stampSpacing));
    int t1r = int(floor((i_startArcLength + t1) / stampSpacing));
    for (int i = t0r; i < t1r; ++i) {
        float t = i * stampSpacing - i_startArcLength;
        vec2 uv = vec2(remap(xp - t, 0.0, r0, 0., texSize), remap(y, -h, h, 0., texSize));
        float I = imageLoad(tex, ivec2(uv)).r;
        alpha += I;
    }
    #endif  // STAMPED

    #endif  // AIRBRUSH_TEST


    #ifdef TEXTURED
    uint brushIndex = i_brush;
    #ifdef TEXTURE_OVERRIDE
    brushIndex = u.brush;
    #endif
    image2DHandle tex = u.brushTextures.d[brushIndex];
    const float texSize = 256;
    float v = remap(clamp(y, -h, h), -h, h, 0.0, texSize-1.);
    const float stamp_scale = 0.5;
    float param = 0.6;
    vec2 u_range = vec2(max(0, min(stamp_scale - 1.0 + param, stamp_scale)), min(param, stamp_scale)) / stamp_scale;
    //vec2 u_range = vec2(0, 1);
    ivec2 A = ivec2((texSize-1.) * 0.0, v);
    ivec2 B = ivec2((texSize-1.) * 1.0, v);
    float Ia = imageLoad(tex, A).r;
    float Ib = imageLoad(tex, B).r;
    float integral = 2.0 * (Ib - Ia) / texSize;
    alpha *= integral;
    #endif



    #ifdef OPACITY_OVERRIDE
    alpha *= OPACITY_OVERRIDE;
    #endif


    o_color = i_color * vec4(1., 1., 1., alpha);
    //o_color = vec4(0.0, v / texSize, 0., 1.0);

    #ifdef SHOW_ARCLENGTH
    o_color = vec4(palette(
        i_pixelPosition.x / 100.,
        vec3(0.5, 0.5, 0.5),
        vec3(0.5, 0.5, 0.5),
        vec3(1.0, 1.0, 1.0),
        vec3(0.00, 0.33, 0.67)), alpha);
    #endif

    #ifdef PROCEDURAL_BRUSH_TIP
    // The brush tip is an ellipse
    //vec2 line
    vec2 tipSize = vec2(0.3*h, h);  // ellipse semiaxes in pixels
    float l = intEllipse(i_position.y * i_normal, vec2(i_normal.y, -i_normal.x), tipSize);
    l = 1.0 - exp(-l * h);
    o_color *= vec4(1., 1., 1., l / h);
    #endif

    #ifdef DITHER
    o_color.rgb += vec3(0.01 * noise(i_pixelPosition + 10000.0 * u.sceneParams.d.time));
    #endif

    #ifdef SHOW_STROKE_SKELETON
    o_color += 0.1 * vec4(1. - smoothstep(0.5, 1.5, abs(abs(i_position.y) - h)));
    o_color += 0.1 * vec4(1. - smoothstep(0.5, 1.5, abs(i_position.y)));
    #endif



    //vec2 dir = * vec2(i_normal.y, -i_normal.x);
    //float dist = distSeg(i_position.y * i_normal -30*dir, i_position.y * i_normal + 30 * vec2(i_normal.y, -i_normal.x)

    //o_color = vec4(i_normal, 0.0, 1.0);
}

#endif