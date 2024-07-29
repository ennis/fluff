#version 460 core
#include "bindless.inc.glsl"
#include "common.inc.glsl"
#include "shared.inc.glsl"
#include "bezier.inc.glsl"

#extension GL_EXT_mesh_shader : require
#extension GL_ARB_fragment_shader_interlock : require
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_KHR_shader_subgroup_ballot : require
#extension GL_KHR_shader_subgroup_arithmetic : require
#extension GL_KHR_shader_subgroup_shuffle : require
#extension GL_EXT_debug_printf : enable

layout(push_constant, scalar) uniform PushConstants {
    BinCurvesParams u;
};

//////////////////////////////////////////////////////////

// Converts normalized device coordinates (NDC) to window coordinates.
vec3 ndc2win(vec3 ndc) {
    return (ndc * .5 + .5) * vec3(vec2(u.viewportSize), 1.);
}

// Converts window coordinates to normalized device coordinates (NDC).
vec3 win2ndc(vec3 window) {
    return window / vec3(vec2(u.viewportSize), 1.) * 2. - 1.;
}

vec4 project(vec3 pos)
{
    vec4 clip = u.sceneParams.d.viewProj * vec4(pos, 1.);
    clip.y = -clip.y;
    return vec4(ndc2win(clip.xyz/clip.w), clip.w);
}

vec4 projectDir(vec3 dir)
{
    vec4 clip = u.sceneParams.d.viewProj * vec4(dir, 0.);
    clip.y = -clip.y;
    return vec4(ndc2win(clip.xyz/clip.w), clip.w);
}

//////////////////////////////////////////////////////////////////////

//const int SUBDIV_LEVEL_OFFSET = 2; // minimum 4 subdivisions
//const int SUBDIV_LEVEL_COUNT = 4; // 4,8,16,32
//const int MAX_VERTICES_PER_CURVE = 64;

const uint SUBGROUP_SIZE = BINPACK_SUBGROUP_SIZE;

#ifndef DEGENERATE_CURVE_THRESHOLD
const float DEGENERATE_CURVE_THRESHOLD = 0.01;
#endif

struct PackedCurve {
    uint8_t curveID; // relative
    uint8_t subdiv;     // (4,8,16,32) number of points in the subdivided curve
    uint16_t wgroupID;
};

// Shared task payload.
struct TaskData {
    uint baseCurveID;
    // 8 bits  : curve index
    // 8 bits  : subdivision level (0-3)
    // 16 bits : meshlet offset
    PackedCurve curves[SUBGROUP_SIZE];
};


#ifdef __TASK__

layout(local_size_x=SUBGROUP_SIZE) in;

taskPayloadSharedEXT TaskData taskData;

// Inspired by https://github.com/nvpro-samples/vk_displacement_micromaps/blob/main/micromesh_binpack.glsl
// Returns the total number of workgroups to launch for the given bin.
uint binPack(uint subdiv) {
    uint taskwgroupID = gl_WorkGroupID.x;
    uint laneID = gl_SubgroupInvocationID;

    uvec4 vote;
    if (subdiv == 4) {
        vote = subgroupBallot(true);
    } else if (subdiv == 8) {
        vote = subgroupBallot(true);
    } else if (subdiv == 16) {
        vote = subgroupBallot(true);
    } else if (subdiv == 32) {
        vote = subgroupBallot(true);
    } else if (subdiv == 64) {
        vote = subgroupBallot(true);
    }

    uint binFirst = subgroupBallotFindLSB(vote);
    //uint packLast = subgroupBallotFindMSB(vote);
    uint binSize = subgroupBallotBitCount(vote);
    uint binPrefix = subgroupBallotExclusiveBitCount(vote);

    bool isBinFirst = binFirst == laneID;
    uint binStartIdx = subgroupExclusiveAdd(isBinFirst ? binSize : 0);
    binStartIdx = subgroupShuffle(binStartIdx, binFirst);

    uint outID = binStartIdx + binPrefix;

    // One pack == one mesh workgroup processing a coherent vertex workload
    // e.g.
    // - (0.5x64) 32 vertices for a curve with 64 subdivisions
    // - 1x32 vertices for a curve with 32 subdivisions
    // - 2x16 vertices for two curves with 16 subdivisions
    // - 4x8 vertices for four curves with 8 subdivisions
    // - 8x4 vertices for eight curves with 4 subdivisions

    // lane   : 0  6  7 10 15  2  4  5  1 11 13  3 12  14   8   9
    // subd   : 4  4  4  4  4  8  8  8  8  8  8 16 16  16  16  16
    // scan   : 4  8 12 16 20 28 36 44 52 60 68 84 100 116 132
    // wgroup : 0  0  0  0  0  0  1  1  1  1  2  2  3   3   4   4

    // Determine number of workgroups for the bin (or equivalently the number of meshlets
    // since each workgroup produces one meshlet).
    uint threadCount = subdiv;
    uint wgroupCount = (binSize * threadCount + SUBGROUP_SIZE - 1) / SUBGROUP_SIZE;

    // First workgroup of the bin
    uint wgroupOffset = subgroupExclusiveAdd(isBinFirst ? wgroupCount : 0);
    wgroupOffset = subgroupShuffle(wgroupOffset, binFirst);

    uint wgroupID = wgroupOffset + (binPrefix * threadCount) / SUBGROUP_SIZE;
    taskData.curves[outID] = PackedCurve(uint8_t(laneID), uint8_t(subdiv), uint16_t(wgroupID));

    uint wgroupTotalCount = subgroupAdd(isBinFirst ? wgroupCount : 0);

    if (isBinFirst && taskwgroupID == 0) {
        debugPrintfEXT("Subdiv level %d: binSize=%d, binStartIdx=%d, binPrefix=%d, wgroupCount=%d, wgroupID(first)=%d\n", subdiv, binSize, binStartIdx, binPrefix, wgroupCount,  wgroupID);
    }

    return wgroupTotalCount;
}

void main() {
    uint curveIdx = gl_GlobalInvocationID.x;

    // Determine if this invocation is valid, or if this is a slack invocation due to the number of curves not being
    // a multiple of the workgroup size, in which case this invocation should do nothing.
    // NOTE: don't return early because there is a barrier() call below that must be executed in uniform control flow.
    bool enabled = curveIdx < u.curveCount;
    curveIdx = min(curveIdx, u.curveCount - 1);

    uint cpIdx = u.curves.d[u.baseCurveIndex + curveIdx].start;
    vec3 p0 = project(u.controlPoints.d[cpIdx + 0].pos).xyz;
    vec3 p1 = project(u.controlPoints.d[cpIdx + 1].pos).xyz;
    vec3 p2 = project(u.controlPoints.d[cpIdx + 2].pos).xyz;
    vec3 p3 = project(u.controlPoints.d[cpIdx + 3].pos).xyz;

    // Determine the subdivision count of the curve.
    // Adapted from https://scholarsarchive.byu.edu/cgi/viewcontent.cgi?article=1000&context=facpub#section.10.6
    vec3 ddv = 6.0 * max(abs(p3 - 2.0 * p2 + p1), abs(p2 - 2.0 * p1 + p0));
    //debugPrintfEXT("ddv = %f %f %f\n", ddv.x, ddv.y, ddv.z);
    const float tolerance = 1.0;
    int n = int(ceil(sqrt(0.25 * length(ddv) / tolerance)));
    // Clamp the maximum subdivision level, and round to next power of 2
    n = clamp(n, 4, 64);
    n = 1 << 1 + findMSB(n - 1);    // n = 4,8,16 or 32

    taskData.baseCurveID = u.baseCurveIndex + gl_WorkGroupID.x * SUBGROUP_SIZE;

    if (enabled) {
        uint wgroupCount = binPack(n);
        if (gl_LocalInvocationIndex == 0) {
            EmitMeshTasksEXT(wgroupCount, 1, 1);
        }
    }
}

#endif

//////////////////////////////////////////////////////////

#ifdef __MESH__

layout(local_size_x=SUBGROUP_SIZE) in;

// Output: triangle mesh tessellation of the curve
// 2 vertices per sample, 2 triangles subdivision
layout(triangles, max_vertices=2*SUBGROUP_SIZE, max_primitives=2*(SUBGROUP_SIZE - 1)) out;
layout(location=0) flat out int[] o_curveIndex;
layout(location=1) out vec2[] o_uv;
layout(location=2) out vec3[] o_color;
layout(location=3) perprimitiveEXT out vec4[] o_line;

shared vec4[MAX_VERTICES_PER_CURVE] s_curvePositions;
//shared vec3[MAX_VERTICES_PER_CURVE] s_curveColors;
//shared vec2[MAX_VERTICES_PER_CURVE] s_curveNormals;

taskPayloadSharedEXT TaskData taskData;

void binUnpack(uint wgroupID,
            out uint curveID,
            out uint packThreads,    // subdivision point count (4,8,16,32)
            out uint packSize,       // how many curves processed in parallel by this workgroup
            out uint packID,         // which curve among the ones processed in parallel (0..7)
            out uint packThreadID,   // 0..packThreads-1
            out bool valid)
{
    uint laneID = gl_SubgroupInvocationID;
    uvec4 vote = subgroupBallot(wgroupID >= taskData.curves[laneID].wgroupID);
    uint packStart = subgroupBallotFindMSB(vote);
    packThreads = taskData.curves[packStart].subdiv;
    uint wgroupStart = taskData.curves[packStart].wgroupID;
    packThreadID = ((wgroupID - wgroupStart) * SUBGROUP_SIZE + laneID) % packThreads;
    packID = laneID / packThreads;
    curveID = taskData.baseCurveID + taskData.curves[packStart + packID].curveID;
    valid = taskData.curves[packStart + packID].subdiv == packThreads;
    uvec4 allValids = subgroupBallot((packThreadID == 0) && valid);
    packSize = subgroupBallotBitCount(allValids);
}


void main()
{
    uint laneID = gl_SubgroupInvocationID;  // It's also gl_LocalInvocationIndex

    uint curveID;
    uint packSize;
    uint packID;
    uint packThreads;
    uint packThreadID;
    bool valid;
    binUnpack(gl_WorkGroupID.x, curveID, packThreads, packSize, packID, packThreadID, valid);

   //if (gl_WorkGroupID.x == 0) {
   //    debugPrintfEXT("(wgroupID=%2d,laneID=%2d) curveID=%4d, packSize=%2d, packThreads=%2d, packThreadID=%2d, packID=%d, valid=%d\n",
   //                gl_WorkGroupID.x, laneID,
   //                curveID,
   //                packSize,
   //                packThreads,
   //                packThreadID,
   //                packID,
   //                valid);
   //}

    float t = float(packThreadID) / float(packThreads - 1);
    CurveDesc curve = u.curves.d[curveID];

    // whether this invocation is the first sample of a curve segment
    bool isFirst = (packThreadID == 0) || (laneID == 0);
    // whether this invocation is the last sample of a curve segment
    bool isLast = (packThreadID == packThreads - 1) || (laneID == SUBGROUP_SIZE - 1);

    // Evaluate the position on the curve at the current t parameters
    // and store them in shared memory.
    vec3 color;
    if (valid) {
        ControlPoint cp0 = u.controlPoints.d[curve.start];
        ControlPoint cp1 = u.controlPoints.d[curve.start + 1];
        ControlPoint cp2 = u.controlPoints.d[curve.start + 2];
        ControlPoint cp3 = u.controlPoints.d[curve.start + 3];
        vec4 p0 = u.sceneParams.d.viewProj * vec4(cp0.pos, 1.);
        vec4 p1 = u.sceneParams.d.viewProj * vec4(cp1.pos, 1.);
        vec4 p2 = u.sceneParams.d.viewProj * vec4(cp2.pos, 1.);
        vec4 p3 = u.sceneParams.d.viewProj * vec4(cp3.pos, 1.);
        p0.y = -p0.y;
        p1.y = -p1.y;
        p2.y = -p2.y;
        p3.y = -p3.y;
        RCubicBezier3D curve = RCubicBezier3D(p0, p1, p2, p3);
        s_curvePositions[laneID] = evalRCubicBezier3D(curve, t);
        color = mix(cp0.color, cp3.color, t);
    }

    barrier();

    if (valid) {
        // Compute normal
        vec4 pprev = s_curvePositions[isFirst ? laneID : laneID - 1];
        vec4 p0    = s_curvePositions[laneID];
        vec4 pnext = s_curvePositions[isLast ? laneID : laneID + 1];
        vec2 vprev = normalize((p0.xy/p0.w - pprev.xy/pprev.w) * u.viewportSize);
        vec2 vnext = normalize((pnext.xy/pnext.w - p0.xy/p0.w) * u.viewportSize);
        vec2 v = 0.5 * (vprev + vnext);
        vec2 n = vec2(-v.y, v.x) / u.viewportSize;

        // Conservative rasterization factor
        const float conservative = 8. * sqrt(2.);
        float hw_aa = u.strokeWidth + conservative;

        // Emit geometry
        uint vertexCount = 2 * packSize * packThreads;
        uint primCount = 2 * (packSize * packThreads - 1);
        SetMeshOutputsEXT(vertexCount, primCount);

        vec4 a = p0;
        vec4 b = p0;
        a.xy -= n * hw_aa * a.w;
        b.xy += n * hw_aa * b.w;

        // Modulo SUBGROUP_SIZE because we can have packThreadID=32..63 and SUBGROUP_SIZE=32
        // for curves with 64 subdivisions split across two meshlets.
        uint primIdx = (packID * packThreads + packThreadID) % SUBGROUP_SIZE;
        uint vertIdx = primIdx * 2;

        /*if (gl_WorkGroupID.x == 0) {
            debugPrintfEXT("(wgroupID=%2d,laneID=%2d) curveID=%4d, packSize=%2d, packThreads=%2d, packThreadID=%2d, packID=%d, valid=%d, primIdx=%2d, vertIdx=%2d\n",
            gl_WorkGroupID.x, laneID,
            curveID,
            packSize,
            packThreads,
            packThreadID,
            packID,
            valid,primIdx,vertIdx);
        }*/

        gl_MeshVerticesEXT[vertIdx].gl_Position = a;
        gl_MeshVerticesEXT[vertIdx + 1].gl_Position = b;
        o_curveIndex[vertIdx] = int(curveID);
        o_curveIndex[vertIdx + 1] = int(curveID);
        o_uv[vertIdx] = vec2(remap(t, 0., 1., curve.paramRange.x, curve.paramRange.y), hw_aa);
        o_uv[vertIdx + 1] = vec2(remap(t, 0., 1., curve.paramRange.x, curve.paramRange.y), -hw_aa);
        o_color[vertIdx] = color;
        o_color[vertIdx + 1] = color;

        //(ndc * .5 + .5) * vec3(vec2(u.viewportSize), 1.);

        vec4 line = (vec4(p0.xy/p0.w, pnext.xy/pnext.w) + 1.) * 0.5 * u.viewportSize.xyxy;
        o_line[primIdx] = line;
        o_line[primIdx+1] = line;

        if (!isLast) {
            gl_PrimitiveTriangleIndicesEXT[vertIdx] = uvec3(vertIdx, vertIdx+2, vertIdx+1);
            gl_PrimitiveTriangleIndicesEXT[vertIdx+1] = uvec3(vertIdx+1, vertIdx+2, vertIdx+3);

            /*vec4 a = s0.p.xy + s0.n * hw_aa - s0.t * conservative;
            vec4 b = s0.p.xy - s0.n * hw_aa - s0.t * conservative;
            vec2 c = s1.p.xy + s1.n * hw_aa + s1.t * conservative;
            vec2 d = s1.p.xy - s1.n * hw_aa + s1.t * conservative;

            gl_MeshVerticesEXT[localIndex*4 + 0].gl_Position = a;
            gl_MeshVerticesEXT[localIndex*4 + 1].gl_Position = b;
            gl_MeshVerticesEXT[localIndex*4 + 2].gl_Position = c;
            gl_MeshVerticesEXT[localIndex*4 + 3].gl_Position = d;
            o_curveIndex[localIndex*4 + 2] = int(curveIndex);
            o_curveIndex[localIndex*4 + 3] = int(curveIndex);

            vec2 pr = u.curves.d[u.baseCurveIndex + curveIndex].paramRange;
            o_uv[localIndex*4 + 0] = vec2(remap(t, 0., 1., pr.x, pr.y), hw_aa);
            o_uv[localIndex*4 + 1] = vec2(remap(t, 0., 1., pr.x, pr.y), -hw_aa);
            o_uv[localIndex*4 + 2] = vec2(remap(t, 0., 1., pr.x, pr.y), hw_aa);
            o_uv[localIndex*4 + 3] = vec2(remap(t, 0., 1., pr.x, pr.y), -hw_aa);

            vec3 cd = u.controlPoints.d[u.curves.d[u.baseCurveIndex + curveIndex].start].color;
            o_color[localIndex*4 + 2] = cd;
            o_color[localIndex*4 + 3] = cd;*/
        } else {
            //gl_PrimitiveTriangleIndicesEXT[invocation*2] = uvec3(0, 0, 0);
            //gl_PrimitiveTriangleIndicesEXT[invocation*2+1] = uvec3(0, 0, 0);
        }
    }
}

#endif  // __MESH__

//////////////////////////////////////////////////////////

#ifdef __FRAGMENT__

layout(location=0) flat in int i_curveIndex;
layout(location=1) in vec2 i_uv;
layout(location=2) in vec3 i_color;
layout(location=3) perprimitiveEXT in vec4 i_line;
layout(location=0) out vec4 o_color;

void main() {
    uint tileIndex = uint(gl_FragCoord.x) + uint(gl_FragCoord.y) * u.tileCountX;

    uint count = atomicAdd(u.tileLineCount.d[tileIndex], 1);
    if (count < MAX_LINES_PER_TILE) {
        u.tileData.d[tileIndex].lines[count].coords = i_line;
        u.tileData.d[tileIndex].lines[count].curveIndex = i_curveIndex;
    }

    o_color = vec4(0., 1., 0., 0.2);
}

#endif