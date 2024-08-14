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

    //packThreadID = binThreadID % packThreads;
    //packID = binThreadID / packThreads;  // index of the packed curve in this mesh workgroup


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
           // cfg.cull = true;
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

        vec4 p = project(v.pos);
        vec4 p0 = project(v0.pos);
        vec4 p1 = project(v1.pos);

        // half-width + anti-aliasing margin
        // clamp the width to 1.0, below that we just fade out the line
        float hwAA = max(u.width, 1.0) * 0.5 + u.filterWidth * sqrt(2.0);
        vec2 pxSize = vec2(2) / u.sceneParams.d.viewportSize; // pixel size in clip space

        // compute normals
        vec2 n;
        if (isFirst) {
            vec2 v = p1.xy/p1.w - p.xy/p.w;
            n = hwAA * pxSize * normalize(pxSize * vec2(-v.y, v.x));

        } else if (isLast) {
            vec2 v = p.xy/p.w - p0.xy/p0.w;
            n = hwAA * pxSize * normalize(pxSize * vec2(-v.y, v.x));
        }
        else {
            vec2 v0 = normalize((p.xy/p.w - p0.xy/p0.w) / pxSize);
            vec2 v1 = normalize((p1.xy/p1.w - p.xy/p.w) / pxSize);
            vec2 vt = 0.5 * (v0 + v1);
            n = vec2(-vt.y, vt.x);
            // half-width / sin(theta/2)
            float d = hwAA / max(cross(vec3(v0, 0.0), vec3(n, 0.0)).z, 0.05);
            n = d * n * pxSize;
        }

        vec4 a = p;
        vec4 b = p;
        a.xy -= n * a.w;
        b.xy += n * b.w;


        uint voff = cfg.meshletVtxOffset;
        gl_MeshVerticesEXT[voff].gl_Position = a;
        gl_MeshVerticesEXT[voff+1].gl_Position = b;
        o_color[voff+0] = vec4(v.color) / 255.0;
        o_color[voff+1] = vec4(v.color) / 255.0;
        #ifdef HIGHLIGHT_LONG_STROKES
        // < 32: green
        // 32-64: yellow
        // 64-128: orange
        // 128-256: red
        // > 256: magenta
        vec4 color;
        if (cfg.vertexCount < SUBGROUP_SIZE) {
            color = vec4(0.0, 1.0, 0.0, 1.0);
        }
        else if (cfg.vertexCount < 2*SUBGROUP_SIZE) {
            color = vec4(1.0, 1.0, 0.0, 1.0);
        }
        else if (cfg.vertexCount < 4*SUBGROUP_SIZE) {
            color = vec4(1.0, 0.3, 0.0, 1.0);
        }
        else if (cfg.vertexCount < 8*SUBGROUP_SIZE) {
            color = vec4(1.0, 0.0, 0.0, 1.0);
        }
        else {
            color = vec4(1.0, 0.0, 1.0, 1.0);
        }
        o_color[voff+0] = color;
        o_color[voff+1] = color;
        #endif
        o_position[voff+0] = vec2(0.0, -hwAA);
        o_position[voff+1] = vec2(1.0, hwAA);

        if (isLast) {
            debugPrintfEXT("strokeID=%4d, vertexIdx=%4d, vertexCount=%4d, meshletVtxCount=%4d, meshletPrimCount=%4d, meshletVtxOffset=%4d\n", cfg.strokeID, cfg.vertexIdx, cfg.vertexCount, cfg.meshletVtxCount, cfg.meshletPrimCount, cfg.meshletVtxOffset);
        }

        if (cfg.meshletPrimOffset < cfg.meshletPrimCount - 1) {
            uint poff = cfg.meshletPrimOffset;
            gl_PrimitiveTriangleIndicesEXT[poff] = uvec3(voff, voff+2, voff+1);
            gl_PrimitiveTriangleIndicesEXT[poff+1] = uvec3(voff+1, voff+2, voff+3);
            gl_MeshPrimitivesEXT[poff].gl_CullPrimitiveEXT = isLast;
            gl_MeshPrimitivesEXT[poff+1].gl_CullPrimitiveEXT = isLast;
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
layout(location=0) out vec4 o_color;

void main() {
    // clamped width
    float width = u.width;
    float width1 = max(width, 1.0);
    float h = width1 * 0.5;
    float y = abs(i_position.y);
    //float filterWidth = 1.5;
    float halfFilterWidth = u.filterWidth * 0.5;
    float alpha = (clamp((y + h + halfFilterWidth), 0., width1) - clamp((y + h - halfFilterWidth), 0., width1)) / u.filterWidth;
    alpha *= min(width, 1.0);
    o_color = i_color * vec4(1., 1., 1., alpha);
}

#endif