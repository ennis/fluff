#version 460 core

#include "bindless.inc.glsl"
#include "shared.inc.glsl"


#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_mesh_shader : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

//const uint POLYLINE_START = 1;
//const uint POLYLINE_END = 2;
//const uint POLYLINE_SCREEN_SPACE = 4;

//const uint MAX_POLYLINE_VERTICES = 16384;
//const uint MESH_WORKGROUP_SIZE = 32;

//////////////////////////////////////////////////////////

struct StrokeVertex {
    vec3 position;
    u8vec4 color;
};

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
//


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
    StrokePushConstants u;
    /*mat4 viewProjectionMatrix;
    uint vertexCount;
    float width;
    float filterWidth;
    float screenWidth;
    float screenHeight;*/
};

//////////////////////////////////////////////////////////

#ifdef __TASK__

layout(local_size_x=SUBGROUP_SIZE) in;

taskPayloadSharedEXT TaskData taskData;


// Inspired by https://github.com/nvpro-samples/vk_displacement_micromaps/blob/main/micromesh_binpack.glsl
// Returns the total number of workgroups to launch.
uint binPack(uint strokeVertexCount) {
    uint laneID = gl_SubgroupInvocationID;

    uvec4 vote;
    uint vertexCount = roundUpPow2(strokeVertexCount);

    // This can be replaced with a loop or ideally subgroupPartitionNV if available
    if (vertexCount == 4) {
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
    }else if (vertexCount == 256) {
        vote = subgroupBallot(true);
    }else if (vertexCount == 512) {
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

    // Process 32 (SUBGROUP_SIZE) strokes at once,
    uint strokeID = gl_GlobalInvocationID.x;
    if (strokeID < u.strokeCount) {
        uint wgroupCount = binPack(u.strokes.d[strokeID].vtxCount);
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
    return u.sceneParams.d.viewProj * vec4(pos, 1.0);
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

    cfg.vertexIdx = binThreadID % cfg.vertexCountPow2;
    cfg.partIdx = cfg.vertexIdx / SUBGROUP_SIZE;
    uint binStrokeIdx = binThreadID / cfg.vertexCountPow2;

    //packThreadID = binThreadID % packThreads;
    //packID = binThreadID / packThreads;  // index of the packed curve in this mesh workgroup

    uint inID = binStart + binStrokeIdx;

    // check for overflow and that we're not reading the next bin
    cfg.valid = inID < SUBGROUP_SIZE && taskData.strokes[inID].log2VertexCount == log2VertexCount;

    if (cfg.valid) {
        cfg.strokeID = taskData.baseStrokeID + taskData.strokes[inID].strokeIdx;
        cfg.vertexCount = u.strokes[cfg.strokeID].vertexCount;
        // check that we're not going beyond the stroke vertex count
        if (cfg.vertexIdx >= cfg.vertexCount) {
            cfg.valid = false;
        }
    }

    if (cfg.valid) {
        const uint maxVertsPerMeshlet = SUBGROUP_SIZE * 2;
        cfg.meshletVtxOffset = binThreadID % maxVertsPerMeshlet;
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

        Stroke stroke = u.strokes[cfg.strokeID];
        uint vertex = stroke.baseVertex + cfg.vertexIdx;

        StrokeVertex v = u.vertices.d[vertex];
        StrokeVertex v0 = isFirst ? v : u.vertices.d[vertex - 1];
        StrokeVertex v1 = isLast ? v : u.vertices.d[vertex + 1];

        vec4 p = project(v.position);
        vec4 p0 = project(v0.position);
        vec4 p1 = project(v1.position);

        // half-width + anti-aliasing margin
        // clamp the width to 1.0, below that we just fade out the line
        float hwAA = max(u.width, 1.0) * 0.5 + u.filterWidth * sqrt(2.0);
        vec2 pxSize = vec2(2. / u.screenWidth, 2. / u.screenHeight); // pixel size in clip space

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
        o_position[voff+0] = vec2(0.0, -hwAA);
        o_position[voff+1] = vec2(1.0, hwAA);

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
    float width1 = max(width, 1.0);
    float h = width1 * 0.5;
    float y = abs(i_position.y);
    //float filterWidth = 1.5;
    float halfFilterWidth = filterWidth * 0.5;
    float alpha = (clamp((y + h + halfFilterWidth), 0., width1) - clamp((y + h - halfFilterWidth), 0., width1)) / filterWidth;
    alpha *= min(width, 1.0);
    o_color = i_color * vec4(1., 1., 1., alpha);
}

#endif