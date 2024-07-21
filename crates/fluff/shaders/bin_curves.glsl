#version 460 core
#include "bindless.inc.glsl"
#include "common.inc.glsl"
#include "shared.inc.glsl"
#include "bezier.inc.glsl"

#extension GL_EXT_mesh_shader : require
#extension GL_ARB_fragment_shader_interlock : require
#extension GL_EXT_nonuniform_qualifier : require


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

uint loadCurveFirstCP(uint curveIndex)
{
    return u.curves.d[u.baseCurveIndex + curveIndex].start;
}

// Load a bezier segment from an index into the control point buffer
RationalCubicBezier3DSegment loadProjectedCubicBezierSegment(uint curveIndex)
{
    uint firstCP = loadCurveFirstCP(curveIndex);
    vec4 proj0 = project(u.controlPoints.d[firstCP + 0].pos);
    vec4 proj1 = project(u.controlPoints.d[firstCP + 1].pos);
    vec4 proj2 = project(u.controlPoints.d[firstCP + 2].pos);
    vec4 proj3 = project(u.controlPoints.d[firstCP + 3].pos);
    return RationalCubicBezier3DSegment(proj0.xyz, proj1.xyz, proj2.xyz, proj3.xyz, proj0.w, proj1.w, proj2.w, proj3.w);
}

const int SUBDIV_LEVEL_OFFSET = 2; // minimum 4 subdivisions
const int SUBDIV_LEVEL_COUNT = 4; // 4,8,16,32

//const int MAX_VERTICES_PER_CURVE = 64;
const uint MAX_CURVES_PER_SUBDIV_LEVEL = BINNING_TASK_WORKGROUP_SIZE;

#ifndef DEGENERATE_CURVE_THRESHOLD
const float DEGENERATE_CURVE_THRESHOLD = 0.01;
#endif

// Returns the number of points of a subdivided curve at the given level.
uint subdivPointCount(uint level) {
    return (1 << level + SUBDIV_LEVEL_OFFSET) + 1;
}

// Returns the number of curves that can be processed in parallel by a single mesh workgroup for a given subdivision level.
//
// Sample values for MAX_VERTICES_PER_CURVE = 64:
// - level 0: 12 curves
// - level 1: 64 / 9 = 7 curves
// - bucket 2: 64 / 17 = 3 curves
// - bucket 3: 64 / 33 = 1 curve (poor occupancy)
uint subdivCurvesPerWorkgroup(uint level) {
    return MAX_VERTICES_PER_CURVE / subdivPointCount(level);
}

// Shared task payload.
// FIXME: this is too large, according to nvidia's recommendations.
struct TaskData {
    // Number of curves in each subdivision bin.
    uint countForBin[SUBDIV_LEVEL_COUNT];
    // Curve indices for each subdivision bin.
    uint subdivBins[SUBDIV_LEVEL_COUNT*MAX_CURVES_PER_SUBDIV_LEVEL];
};

#ifdef __TASK__

layout(local_size_x=BINNING_TASK_WORKGROUP_SIZE) in;

taskPayloadSharedEXT TaskData taskData;

void main() {

    uint curveIndex = gl_GlobalInvocationID.x;

    // Initialize the task payload struct.
    if (gl_LocalInvocationIndex < SUBDIV_LEVEL_COUNT) {
        taskData.countForBin[gl_LocalInvocationIndex] = 0;
    }

    // Wait for all invocations to finish clearing the task payload.
    barrier();

    // Determine if this invocation is valid, or if this is a slack invocation due to the number of curves not being
    // a multiple of the workgroup size, in which case this invocation should do nothing.
    // NOTE: don't return early because there is a barrier() call below that must be executed in uniform control flow.
    bool enabled = curveIndex < u.curveCount;
    //curveIndex = min(curveIndex, curveCount - 1);

    // View direction vector
    //vec3 viewDir = normalize(u.sceneParams[3].xyz);

    if (enabled) {
        // Load the control points of the curve
        uint firstCP = loadCurveFirstCP(curveIndex);
        vec3 p_0 = (u.sceneParams.d.view * vec4(u.controlPoints.d[firstCP + 0].pos, 1.0)).xyz;
        vec3 p_1 = (u.sceneParams.d.view * vec4(u.controlPoints.d[firstCP + 1].pos, 1.0)).xyz;
        vec3 p_2 = (u.sceneParams.d.view * vec4(u.controlPoints.d[firstCP + 2].pos, 1.0)).xyz;
        vec3 p_3 = (u.sceneParams.d.view * vec4(u.controlPoints.d[firstCP + 3].pos, 1.0)).xyz;

        // Assume that the curve is mostly planar, retrieve the plane normal from the first 3 control points.
        vec3 n = normalize(cross(p_1 - p_0, p_2 - p_0));

        // If the curve is aligned with the view direction, remove it (it points towards the camera, so the curve
        // doesn't have a meaningful footprint on screen).
        if (abs(n.z) > DEGENERATE_CURVE_THRESHOLD) {
            // Load and project the cubic Bézier segment
            RationalCubicBezier3DSegment segment = loadProjectedCubicBezierSegment(curveIndex);

            // Determine the subdivision count of the curve.
            // Adapted from https://scholarsarchive.byu.edu/cgi/viewcontent.cgi?article=1000&context=facpub#section.10.6
            float ddx = 6.0 * max(abs(segment.p3.x - 2.0 * segment.p2.x + segment.p1.x), abs(segment.p2.x - 2.0 * segment.p1.x + segment.p0.x));
            float ddy = 6.0 * max(abs(segment.p3.y - 2.0 * segment.p2.y + segment.p1.y), abs(segment.p2.y - 2.0 * segment.p1.y + segment.p0.y));
            float ddz = 6.0 * max(abs(segment.p3.z - 2.0 * segment.p2.z + segment.p1.z), abs(segment.p2.z - 2.0 * segment.p1.z + segment.p0.z));
            const float tolerance = 1.0;
            float dd = length(vec3(ddx, ddy, ddz));
            int n = int(ceil(sqrt(0.25 * dd / tolerance)));

            // Clamp the maximum subdivision level, and round to next power of 2
            n = clamp(n, 1 << SUBDIV_LEVEL_OFFSET, 1 << SUBDIV_LEVEL_OFFSET + SUBDIV_LEVEL_COUNT - 1);
            n = 1 << 1 + findMSB(n - 1);

            // add curve to the corresponding subdivision bucket
            uint bucket = findMSB(n >> 2);
            uint index = atomicAdd(taskData.countForBin[bucket], 1);
            taskData.subdivBins[bucket*BINNING_TASK_WORKGROUP_SIZE + index] = curveIndex;
        }
    }

    barrier();

    // Determine the number of mesh workgroups to launch.
    // (sum the number of workgroups to launch for each subdivision bin)
    if (gl_LocalInvocationIndex == 0) {
        uint numWorkgroupsToLaunch = 0;
        for (int i = 0; i < SUBDIV_LEVEL_COUNT; ++i) {
            uint curvesPerWorkgroup = subdivCurvesPerWorkgroup(i);
            uint numWorkgroupsToLaunchForBucket = divCeil(taskData.countForBin[i], curvesPerWorkgroup);
            numWorkgroupsToLaunch += numWorkgroupsToLaunchForBucket;
        }
        EmitMeshTasksEXT(numWorkgroupsToLaunch, 1, 1);
    }
}

#endif

//////////////////////////////////////////////////////////

#ifdef __MESH__

// One thread per sample on the curve
layout(local_size_x=MAX_VERTICES_PER_CURVE) in;

// Output: triangle mesh tessellation of the curve
// 2 vertices per sample, 2 triangles subdivision
layout(triangles, max_vertices=4*MAX_VERTICES_PER_CURVE, max_primitives=2*MAX_VERTICES_PER_CURVE) out;
layout(location=0) flat out int[] o_curveIndex;
layout(location=1) out vec2[] o_uv;
layout(location=2) out vec3[] o_color;
layout(location=3) perprimitiveEXT out vec4[] o_line;

struct CurveSample {
    vec3 p;
    vec2 n;
    vec2 t;
};
shared CurveSample[MAX_VERTICES_PER_CURVE] s_curveSamples;

taskPayloadSharedEXT TaskData taskData;

void main()
{
    uint subdivisionLevel;          // the subdivision level for this workgroup
    uint baseCurveIndexInBin;         // start position of the workgroup in the subdivision bin
    uint numCurvesInWorkgroup;                // number of curves processed in this workgroup
    {
        uint s = 0;
        for (int i = 0; i < SUBDIV_LEVEL_COUNT; ++i) {
            // Number of curves in this bucket
            uint numCurvesInBin = taskData.countForBin[i];
            uint curvesPerWorkgroup = subdivCurvesPerWorkgroup(i);
            uint numWorkgroupsForBucket = divCeil(numCurvesInBin, curvesPerWorkgroup);
            if (gl_WorkGroupID.x < s + numWorkgroupsForBucket) {
                subdivisionLevel = i;
                baseCurveIndexInBin = (gl_WorkGroupID.x - s) * curvesPerWorkgroup;
                numCurvesInWorkgroup = min(curvesPerWorkgroup, numCurvesInBin - baseCurveIndexInBin);
                break;
            }
            s += numWorkgroupsForBucket;
        }
    }

    uint sampleCount = subdivPointCount(subdivisionLevel);
    // which curve among the group is this thread working on (for workgroups that process multiple curves at once)
    uint localCurveOffset = gl_LocalInvocationIndex / sampleCount;
    uint curveIndex = taskData.subdivBins[subdivisionLevel*MAX_CURVES_PER_SUBDIV_LEVEL + baseCurveIndexInBin + localCurveOffset];
    uint invocation = gl_LocalInvocationIndex;
    uint sampleIndex = invocation % sampleCount;

    // Conservative rasterization factor
    const float conservative = 8. * sqrt(2.);
    // Curve parameter
    float t = float(sampleIndex) / float(sampleCount - 1);

    // There might not be enough curves to process for this workgroup and some invocations might read garbage data,
    // so do nothing in this case.
    // As in the task shader, don't return early, because there is a barrier() call below that must be executed
    // in uniform control flow.
    bool enabled = curveIndex < u.curveCount;

    // Evaluate the position, normal, tangents on the curve at the current t parameters
    // and store them in shared memory.
    if (enabled) {
        RationalCubicBezier3DSegment segment = loadProjectedCubicBezierSegment(curveIndex);
        vec3 position = evalRationalCubicBezier3D(segment, t);
        vec2 tangent = normalize(evalRationalCubicBezier3DTangent(segment, t).xy);
        vec2 normal = vec2(-tangent.y, tangent.x);
        s_curveSamples[invocation] = CurveSample(position, normal, tangent);
    }

    barrier();

    if (enabled) {
        float hw_aa = u.strokeWidth + conservative;

        // Emit geometry
        SetMeshOutputsEXT(4 * sampleCount * numCurvesInWorkgroup, 2 * (sampleCount * numCurvesInWorkgroup - 1));

        if (sampleIndex < sampleCount - 1) {
            CurveSample s0 = s_curveSamples[invocation];
            vec2 a = s0.p.xy + s0.n * hw_aa - s0.t * conservative;
            vec2 b = s0.p.xy - s0.n * hw_aa - s0.t * conservative;
            CurveSample s1 = s_curveSamples[invocation+1];
            vec2 c = s1.p.xy + s1.n * hw_aa + s1.t * conservative;
            vec2 d = s1.p.xy - s1.n * hw_aa + s1.t * conservative;
            gl_MeshVerticesEXT[invocation*4 + 0].gl_Position = vec4(win2ndc(vec3(a, s0.p.z)), 1.0);
            gl_MeshVerticesEXT[invocation*4 + 1].gl_Position = vec4(win2ndc(vec3(b, s0.p.z)), 1.0);
            gl_MeshVerticesEXT[invocation*4 + 2].gl_Position = vec4(win2ndc(vec3(c, s1.p.z)), 1.0);
            gl_MeshVerticesEXT[invocation*4 + 3].gl_Position = vec4(win2ndc(vec3(d, s1.p.z)), 1.0);
            o_curveIndex[invocation*4 + 0] = int(curveIndex);
            o_curveIndex[invocation*4 + 1] = int(curveIndex);
            o_curveIndex[invocation*4 + 2] = int(curveIndex);
            o_curveIndex[invocation*4 + 3] = int(curveIndex);

            vec2 pr = u.curves.d[u.baseCurveIndex + curveIndex].paramRange;
            o_uv[invocation*4 + 0] = vec2(remap(t, 0., 1., pr.x, pr.y), hw_aa);
            o_uv[invocation*4 + 1] = vec2(remap(t, 0., 1., pr.x, pr.y), -hw_aa);
            o_uv[invocation*4 + 2] = vec2(remap(t, 0., 1., pr.x, pr.y), hw_aa);
            o_uv[invocation*4 + 3] = vec2(remap(t, 0., 1., pr.x, pr.y), -hw_aa);

            vec3 cd = u.controlPoints.d[u.curves.d[u.baseCurveIndex + curveIndex].start].color;
            o_color[invocation*4 + 0] = cd;
            o_color[invocation*4 + 1] = cd;
            o_color[invocation*4 + 2] = cd;
            o_color[invocation*4 + 3] = cd;

            vec4 line = vec4(s0.p.xy, s1.p.xy);
            o_line[invocation*2] = line;
            o_line[invocation*2+1] = line;

            uint vtx = invocation*4;
            gl_PrimitiveTriangleIndicesEXT[invocation*2] = uvec3(vtx, vtx+2, vtx+1);
            gl_PrimitiveTriangleIndicesEXT[invocation*2+1] = uvec3(vtx+1, vtx+2, vtx+3);

        } else {
            gl_PrimitiveTriangleIndicesEXT[invocation*2] = uvec3(0, 0, 0);
            gl_PrimitiveTriangleIndicesEXT[invocation*2+1] = uvec3(0, 0, 0);
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