
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
    uint8_t curveID;    // relative
    uint8_t subdiv;     // (4,8,16,32) number of points in the subdivided curve
    uint16_t wgroupMatch;  // if the entry is the first of a bin, this is the first workgroup ID of the bin, otherwise it's the first workgroup ID of the next bin
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

    // wgroup : 0  1  1  1  1  1  2  2  2  2  2  2  3   3   4   4

    // subd    : 64 64 32 32 32 16 16 16 16 16 4  4   4   4
    //           0  2  3  4  5  6  6  7  7  8  9  9   9   9
    //         : 0  2  3  4  5  6  7  7  8  8  9  10  10  10

    // Determine number of workgroups for the bin (or equivalently the number of meshlets
    // since each workgroup produces one meshlet).
    uint threadCount = subdiv;
    uint wgroupCount = (binSize * threadCount + SUBGROUP_SIZE - 1) / SUBGROUP_SIZE;

    // First workgroup of the bin
    uint wgroupMatch = subgroupExclusiveAdd(isBinFirst ? wgroupCount : 0);
    wgroupMatch = subgroupShuffle(wgroupMatch, binFirst);
    // Subtlety: for the first entry in the bin, store the actual first workgroup for the bin,
    // but for others, store the first workgroup of the **next bin**, so that in the mesh shader we can
    // retrieve the first of the bin with `subgroupBallot(wgroupID >= wgroupMatch) / subgroupFindMSB()`.
    wgroupMatch = isBinFirst ? wgroupMatch : wgroupMatch + wgroupCount;

    taskData.curves[outID] = PackedCurve(uint8_t(laneID), uint8_t(subdiv), uint16_t(wgroupMatch));
    uint wgroupTotalCount = subgroupAdd(isBinFirst ? wgroupCount : 0);
    return wgroupTotalCount;
}

void main() {

    uint curveIdx = gl_GlobalInvocationID.x;

    // Initialize task data
    taskData.curves[gl_SubgroupInvocationID] = PackedCurve(uint8_t(0), uint8_t(0), uint16_t(2*SUBGROUP_SIZE));
    subgroupBarrier();

    // Determine if this invocation is valid, or if this is a slack invocation due to the number of curves not being
    // a multiple of the workgroup size, in which case this invocation should do nothing.
    // NOTE: don't return early because there is a barrier() call below that must be executed in uniform control flow.
    bool enabled = curveIdx < u.curveCount;
    //curveIdx = min(curveIdx, u.curveCount - 1);

    if (enabled) {
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
        n = clamp(n, 4, 32);
        n = 1 << 1 + findMSB(n - 1);    // n = 4,8,16 or 32

        uint wgroupCount = binPack(n);

        if (gl_LocalInvocationIndex == 0) {
            EmitMeshTasksEXT(wgroupCount, 1, 1);
        }
    }

    if (gl_SubgroupInvocationID == 0) {
        taskData.baseCurveID = u.baseCurveIndex + gl_WorkGroupID.x * SUBGROUP_SIZE;
    }

   //if (gl_WorkGroupID.x == 0) {
   //    debugPrintfEXT("taskData(%4d.%4d) = { subdiv=%2d, idx=%4d+%2d, wgroupMatch=%d }\n",
   //    gl_WorkGroupID.x,
   //    gl_SubgroupInvocationID,
   //    uint(taskData.curves[gl_SubgroupInvocationID].subdiv),
   //    taskData.baseCurveID,
   //    uint(taskData.curves[gl_SubgroupInvocationID].curveID),
   //    uint(taskData.curves[gl_SubgroupInvocationID].wgroupMatch));
   //}

    subgroupBarrier();

    /*if (taskData.curves[gl_SubgroupInvocationID].curveID + taskData.baseCurveID >= 51802) {
        debugPrintfEXT("INVALID taskData(%4d.%4d) = { subdiv=%2d, idx=%4d+%2d, wgroupMatch=%d }\n",
        gl_WorkGroupID.x,
        gl_SubgroupInvocationID,
        taskData.curves[gl_SubgroupInvocationID].subdiv,
        taskData.baseCurveID,
        taskData.curves[gl_SubgroupInvocationID].curveID,
        taskData.curves[gl_SubgroupInvocationID].wgroupMatch);
    }*/
}

#endif

//////////////////////////////////////////////////////////

#ifdef __MESH__

layout(local_size_x=SUBGROUP_SIZE) in;

// Output: triangle mesh tessellation of the curve
// 2 vertices per sample, 2 triangles subdivision
layout(triangles, max_vertices=4*SUBGROUP_SIZE, max_primitives=4*(SUBGROUP_SIZE - 1)) out;
layout(location=0) perprimitiveEXT flat out uint[] o_curveID;
layout(location=1) perprimitiveEXT out vec4[] o_line;
layout(location=2) perprimitiveEXT out vec2[] o_paramRange;
//layout(location=1) out vec2[] o_uv;
//layout(location=2) out vec3[] o_color;


taskPayloadSharedEXT TaskData taskData;

bool binUnpack(uint wgroupID,
            out uint curveID,
            out uint packThreads,    // subdivision point count (4,8,16,32)
            out uint packSize,       // how many curves processed in parallel by this workgroup
            out uint packID,         // which curve among the ones processed in parallel (0..7)
            out uint packThreadID    // 0..packThreads-1
)
{
    uint laneID = gl_SubgroupInvocationID;
    uint wgroupMatch = taskData.curves[laneID].wgroupMatch;
    uvec4 vote = subgroupBallot(wgroupID >= wgroupMatch);
    if (vote == uvec4(0)) {
        return false;
    }
    uint binStart = subgroupBallotFindMSB(vote);

    packThreads = taskData.curves[binStart].subdiv;     // number of threads per packed curve
    uint wgroupStart = taskData.curves[binStart].wgroupMatch;   // first workgroup of the bin
    uint wgroupOffset = wgroupID - wgroupStart;   // offset of this workgroup in the bin
    uint binThreadID = wgroupOffset * SUBGROUP_SIZE + laneID;
    packThreadID = binThreadID % packThreads;
    packID = binThreadID / packThreads;  // index of the packed curve in this mesh workgroup
    if (binStart + packID >= SUBGROUP_SIZE) {
        // overflow
        return false;
    }
    if (taskData.curves[binStart + packID].subdiv != packThreads) {
        // subdivision level mismatch: we're reading the next bin
        return false;
    }
    uvec4 allValids = subgroupBallot(packThreadID == 0);
    packSize = subgroupBallotBitCount(allValids);   // how many packed curves processed in this workgroup
    curveID = taskData.baseCurveID + taskData.curves[binStart + packID].curveID;
    return true;
}


void main()
{
    uint laneID = gl_SubgroupInvocationID;  // It's also gl_LocalInvocationIndex

    uint curveID;
    uint packSize;
    uint packID;
    uint packThreads;
    uint packThreadID;
    bool valid = binUnpack(gl_WorkGroupID.x, curveID, packThreads, packSize, packID, packThreadID);

   //if (gl_WorkGroupID.x == 0) {
   //
   //    debugPrintfEXT("(wgroupID=%2d,laneID=%2d) curveID=%d, curves[laneID].wgroupID=%4d, packStart=%d, valid=%d\n",
   //    gl_WorkGroupID.x,
   //    laneID,
   //    u.baseCurveIndex + uint(taskData.curves[laneID].curveID),
   //    uint(taskData.curves[laneID].wgroupMatch),
   //    packStart,
   //    uint(valid)
   //    );
   //}

    float t = float(packThreadID) / float(packThreads - 1);
    float tNext = float(packThreadID + 1) / float(packThreads - 1);

    // whether this invocation is the first sample of a curve segment
    bool isFirst = (packThreadID == 0) || (laneID == 0);
    // whether this invocation is the last sample of a curve segment
    bool isLast = (packThreadID == packThreads - 1) || (laneID == SUBGROUP_SIZE - 1);

    // Emit geometry
    uint vertexCount = 4 * packSize * packThreads;
    uint primCount = 2 * (packSize * packThreads - 1);
    SetMeshOutputsEXT(vertexCount, primCount);

    gl_MeshVerticesEXT[4*laneID].gl_Position = vec4(0., 0., 0., 1.);
    //gl_P

    // Evaluate the position on the curve at the current t parameters
    // and store them in shared memory.
    if (valid) {

        uint primOffset = 2 * ((packID * packThreads + packThreadID) % SUBGROUP_SIZE);
        uint vertOffset = 2 * primOffset;

        CurveDesc curve = u.curves.d[curveID];
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

        // Compute the point on the curve, along with tangent and normal.
        // FIXME We compute the tangents/normals of the 2D Bézier curve that is made by projecting the control points of the 3D Bézier
        // onto the screen. This is **not the same curve** as the projection of the 3D Bézier curve (which is a rational
        // Bézier curve), but we use that because it's too complicated to compute the tangent/normal of the rational curve.
        // I'm not sure how far this approximation is from the correct result.
        vec4 pos = evalRCubicBezier3D(RCubicBezier3D(p0, p1, p2, p3), t);
        vec4 posNext = evalRCubicBezier3D(RCubicBezier3D(p0, p1, p2, p3), tNext);
        // screen-space line between this point and the next
        vec2 viewportSize = u.viewportSize;
        vec2 tangent = evalCubicBezier2D_T(CubicBezier2D(p0.xy/p0.w, p1.xy/p1.w, p2.xy/p2.w, p3.xy/p3.w), t);
        //if (length(tangent) < 1e-3) {
        //    tangent = vec2(1., 0.);
        //}
        vec2 tangentN = normalize(viewportSize * tangent);
        vec2 n = vec2(-tangentN.y, tangentN.x) / viewportSize;
        tangentN /= viewportSize;
        if (isnan(n.x) || isnan(n.y)) {
            n = vec2(0., 0.);
            tangentN = vec2(0., 0.);
        }
        // Color
        vec3 color = mix(cp0.color, cp3.color, t);

        // Conservative rasterization factor
        const float conservative = 16. * sqrt(2.);
        float hw_aa = u.strokeWidth + conservative;

        vec4 a = pos;
        vec4 b = pos;
        vec4 c = posNext;
        vec4 d = posNext;
        a.xy += n * hw_aa * a.w - tangentN * hw_aa * a.w;
        b.xy -= n * hw_aa * b.w + tangentN * hw_aa * b.w;
        c.xy += n * hw_aa * c.w + tangentN * hw_aa * c.w;
        d.xy -= n * hw_aa * d.w - tangentN * hw_aa * d.w;

        if (isnan(a.x) || isnan(a.y) || isnan(b.x) || isnan(b.y) || isnan(c.x) || isnan(c.y) || isnan(d.x) || isnan(d.y)) {
            a = vec4(0., 0., 0., 1.);
            b = vec4(0., 0., 0., 1.);
            c = vec4(0., 0., 0., 1.);
            d = vec4(0., 0., 0., 1.);
        }

        if (abs(a.w) > 10000 || abs(b.w) > 10000 || abs(c.w) > 10000 || abs(d.w) > 10000) {
            debugPrintfEXT("a = %f %f %f %f\n", a.x, a.y, a.z, a.w);
            debugPrintfEXT("b = %f %f %f %f\n", b.x, b.y, b.z, b.w);
            debugPrintfEXT("c = %f %f %f %f\n", c.x, c.y, c.z, c.w);
            debugPrintfEXT("d = %f %f %f %f\n", d.x, d.y, d.z, d.w);
        }

        gl_MeshVerticesEXT[vertOffset].gl_Position = a;
        gl_MeshVerticesEXT[vertOffset + 1].gl_Position = b;
        gl_MeshVerticesEXT[vertOffset + 2].gl_Position = c;
        gl_MeshVerticesEXT[vertOffset + 3].gl_Position = d;
        //o_uv[vertOffset] = vec2(remap(t, 0., 1., curve.paramRange.x, curve.paramRange.y), hw_aa);
        //o_uv[vertOffset + 1] = vec2(remap(t, 0., 1., curve.paramRange.x, curve.paramRange.y), -hw_aa);
        //o_uv[vertOffset + 2] = vec2(remap(t, 0., 1., curve.paramRange.x, curve.paramRange.y), hw_aa);
        //o_uv[vertOffset + 3] = vec2(remap(t, 0., 1., curve.paramRange.x, curve.paramRange.y), -hw_aa);
        //o_color[vertOffset] = color;
        //o_color[vertOffset + 1] = color;
        //o_color[vertOffset + 2] = color;
        //o_color[vertOffset + 3] = color;

        if (primOffset < primCount - 1) {
            vec4 line = (vec4(pos.xy/pos.w, posNext.xy/posNext.w) + 1.) * 0.5 * viewportSize.xyxy;
            o_line[primOffset] = line;
            o_line[primOffset+1] = line;
            o_curveID[primOffset] = int(curveID);
            o_curveID[primOffset + 1] = int(curveID);
            vec2 paramRange = vec2(
                remap(t, 0., 1., curve.paramRange.x, curve.paramRange.y),
                remap(tNext, 0., 1., curve.paramRange.x, curve.paramRange.y));
            o_paramRange[primOffset] = paramRange;
            o_paramRange[primOffset + 1] = paramRange;

            gl_PrimitiveTriangleIndicesEXT[primOffset] = uvec3(vertOffset, vertOffset+2, vertOffset+1);
            gl_PrimitiveTriangleIndicesEXT[primOffset+1] = uvec3(vertOffset+1, vertOffset+2, vertOffset+3);
            gl_MeshPrimitivesEXT[primOffset].gl_CullPrimitiveEXT = isLast;
            gl_MeshPrimitivesEXT[primOffset+1].gl_CullPrimitiveEXT = isLast;
        }
    }
}

#endif  // __MESH__

//////////////////////////////////////////////////////////

#ifdef __FRAGMENT__

//layout(location=0) flat in int i_curveIndex;
//layout(location=1) in vec2 i_uv;
//layout(location=2) in vec3 i_color;
layout(location=0) perprimitiveEXT flat in int i_curveID;
layout(location=1) perprimitiveEXT in vec4 i_line;
layout(location=2) perprimitiveEXT in vec2 i_paramRange;
layout(location=0) out vec4 o_color;

void main() {
    uint tileIndex = uint(gl_FragCoord.x) + uint(gl_FragCoord.y) * u.tileCountX;

    // FIXME: this is often called twice for each line (but not always), one for each triangle of the quad; find a way to deduplicate
    uint count = atomicAdd(u.tileLineCount.d[tileIndex], 1);
    if (count < MAX_LINES_PER_TILE) {
        u.tileData.d[tileIndex].lines[count].lineCoords = i_line;
        u.tileData.d[tileIndex].lines[count].curveId = i_curveID;
        u.tileData.d[tileIndex].lines[count].paramRange = i_paramRange;
        u.tileData.d[tileIndex].lines[count].depth = gl_FragCoord.z;
    }

    o_color = vec4(0., 1., 0., 0.2);
}

#endif