#version 460 core
#include "common.glsl"

#extension GL_EXT_mesh_shader : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_ARB_fragment_shader_interlock : require

layout(scalar) buffer;
layout(scalar) uniform;

//////////////////////////////////////////////////////////

// A range of control points in the b_controlPointData.points buffer that describes a single curve.
struct CurveDescriptor {
    vec4 widthProfile; // polynomial coefficients
    vec4 opacityProfile; // polynomial coefficients
    uint start;
    uint size;
    vec2 paramRange;
};

// Per-fixel fragment data before sorting and blending
struct FragmentData {
    vec4 color;
    float depth;
};

//////////////////////////////////////////////////////////

struct ControlPoint {
    vec3 pos;
    vec3 color;
};

buffer CurveControlPoints {
    ControlPoint[] points;
} b_controlPoints;

buffer CurveBuffer {
    CurveDescriptor[] curves;
} b_curveData;

// Push constants
layout(push_constant) uniform PushConstants {
    mat4 viewProjectionMatrix;
    uvec2 viewportSize;
    float strokeWidth;
    int baseCurveIndex;
    int curveCount;
    int tilesCountX;
    int tilesCountY;
    int frame;
};

//////////////////////////////////////////////////////////

// Issue: stroke is drawn in screen space, but fragments should have an associated depth
vec3 ndcToWindow(vec3 ndc) {
    return (ndc * .5 + .5) * vec3(vec2(viewportSize), 1.);
}

vec4 project(vec3 pos)
{
    vec4 p = vec4(pos, 1.);
    vec4 clip = viewProjectionMatrix * vec4(pos, 1.);
    clip.y = -clip.y;
    return vec4(ndcToWindow(clip.xyz/clip.w), clip.w);
}


vec3 windowToNdc(vec3 window) {
    return window / vec3(vec2(viewportSize), 1.) * 2. - 1.;
}

// Load a bezier segment from an index into the control point buffer
RationalCubicBezier3DSegment loadProjectedCubicBezierSegment(uint baseControlPoint) {
    vec3 p0 = b_controlPoints.points[baseControlPoint + 0].pos;
    vec3 p1 = b_controlPoints.points[baseControlPoint + 1].pos;
    vec3 p2 = b_controlPoints.points[baseControlPoint + 2].pos;
    vec3 p3 = b_controlPoints.points[baseControlPoint + 3].pos;

    // Project to screen space
    vec4 p0_proj = project(p0);
    vec4 p1_proj = project(p1);
    vec4 p2_proj = project(p2);
    vec4 p3_proj = project(p3);

    return RationalCubicBezier3DSegment(
        p0_proj.xyz,
        p1_proj.xyz,
        p2_proj.xyz,
        p3_proj.xyz,
        p0_proj.w,
        p1_proj.w,
        p2_proj.w,
        p3_proj.w);

    //vec3 p0 = ndcToWindow(project(b_controlPointData.points[baseControlPoint + segmentIndex*3 + 0]));
    //vec3 p1 = ndcToWindow(project(b_controlPointData.points[baseControlPoint + segmentIndex*3 + 1]));
    //vec3 p2 = ndcToWindow(project(b_controlPointData.points[baseControlPoint + segmentIndex*3 + 2]));
    //vec3 p3 = ndcToWindow(project(b_controlPointData.points[baseControlPoint + segmentIndex*3 + 3]));
    //return CubicBezier3DSegment(p0, p1, p2, p3);
}
struct PositionAndParam {
    vec3 pos;
    float t;
};

const int SUBDIVISION_LEVEL_OFFSET = 2; // minimum 4 subdivisions
const int SUBDIVISION_LEVEL_COUNT = 4; // 4,8,16,32

// NOTE: need to be in sync with application code
const int TASK_WORKGROUP_SIZE = 64;
const int MAX_VERTICES_PER_CURVE = 64;
const int MAX_CURVES_PER_SUBDIVISION_LEVEL = TASK_WORKGROUP_SIZE;


// Returns the number of points of a subdivided curve at the given level.
uint subdivisionPointCount(uint subdivisionLevel) {
    return (1 << subdivisionLevel + SUBDIVISION_LEVEL_OFFSET) + 1;
}

// Returns the number of curves that can be processed in parallel by a single mesh workgroup for a given subdivision level.
//
// Sample values for MAX_VERTICES_PER_CURVE = 64:
// - level 0: 12 curves
// - level 1: 64 / 9 = 7 curves
// - bucket 2: 64 / 17 = 3 curves
// - bucket 3: 64 / 33 = 1 curve (poor occupancy)
uint subdividedCurvesPerMeshWorkgroup(uint subdivisionLevel) {
    return MAX_VERTICES_PER_CURVE / subdivisionPointCount(subdivisionLevel);
}

struct TaskData {
    uint subdivisionBinSizes[SUBDIVISION_LEVEL_COUNT];
    uint subdivisionBins[SUBDIVISION_LEVEL_COUNT*MAX_CURVES_PER_SUBDIVISION_LEVEL];
};

#ifdef __TASK__

layout(local_size_x=TASK_WORKGROUP_SIZE) in;

taskPayloadSharedEXT TaskData taskData;

void main() {
    if (gl_GlobalInvocationID.x >= curveCount) {
        return;
    }

    uint curveIndex = baseCurveIndex + gl_GlobalInvocationID.x;
    RationalCubicBezier3DSegment segment = loadProjectedCubicBezierSegment(b_curveData.curves[curveIndex].start);

    if (gl_LocalInvocationIndex < SUBDIVISION_LEVEL_COUNT) {
        taskData.subdivisionBinSizes[gl_LocalInvocationIndex] = 0;
    }

    barrier();

    // determine subdivision count (https://scholarsarchive.byu.edu/cgi/viewcontent.cgi?article=1000&context=facpub#section.10.6)
    float ddx = 6.0 * max(abs(segment.p3.x - 2.0 * segment.p2.x + segment.p1.x), abs(segment.p2.x - 2.0 * segment.p1.x + segment.p0.x));
    float ddy = 6.0 * max(abs(segment.p3.y - 2.0 * segment.p2.y + segment.p1.y), abs(segment.p2.y - 2.0 * segment.p1.y + segment.p0.y));
    float ddz = 6.0 * max(abs(segment.p3.z - 2.0 * segment.p2.z + segment.p1.z), abs(segment.p2.z - 2.0 * segment.p1.z + segment.p0.z));
    const float tolerance = 1.0;
    float dd = length(vec3(ddx, ddy, ddz));
    int n = int(ceil(sqrt(0.25 * dd / tolerance)));

    // clamp max subdiv level, and round to next power of 2
    n = clamp(n, 1 << SUBDIVISION_LEVEL_OFFSET, 1 << SUBDIVISION_LEVEL_OFFSET + SUBDIVISION_LEVEL_COUNT - 1);
    n = 1 << 1 + findMSB(n - 1);

    // add curve to the corresponding subdivision bucket
    uint bucket = findMSB(n >> 2);
    uint index = atomicAdd(taskData.subdivisionBinSizes[bucket], 1);
    taskData.subdivisionBins[bucket*TASK_WORKGROUP_SIZE + index] = curveIndex;

    barrier();

    // Determine the number of mesh workgroups to launch
    uint numWorkgroupsToLaunch = 0;
    for (int i = 0; i < SUBDIVISION_LEVEL_COUNT; ++i) {
        uint curvesPerWorkgroup = subdividedCurvesPerMeshWorkgroup(i);
        uint numWorkgroupsToLaunchForBucket = div_ceil(taskData.subdivisionBinSizes[i], curvesPerWorkgroup);
        numWorkgroupsToLaunch += numWorkgroupsToLaunchForBucket;
    }

    if (gl_LocalInvocationIndex == 0) {
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
        for (int i = 0; i < SUBDIVISION_LEVEL_COUNT; ++i) {
            // Number of curves in this bucket
            uint numCurvesInBin = taskData.subdivisionBinSizes[i];
            uint curvesPerWorkgroup = subdividedCurvesPerMeshWorkgroup(i);
            uint numWorkgroupsForBucket = div_ceil(numCurvesInBin, curvesPerWorkgroup);
            if (gl_WorkGroupID.x < s + numWorkgroupsForBucket) {
                subdivisionLevel = i;
                baseCurveIndexInBin = (gl_WorkGroupID.x - s) * curvesPerWorkgroup;
                numCurvesInWorkgroup = min(curvesPerWorkgroup, numCurvesInBin - baseCurveIndexInBin);
                break;
            }
            s += numWorkgroupsForBucket;
        }
    }

    uint sampleCount = subdivisionPointCount(subdivisionLevel);
    uint localCurveOffset = gl_LocalInvocationIndex / sampleCount;
    uint curveIndex = taskData.subdivisionBins[subdivisionLevel*MAX_CURVES_PER_SUBDIVISION_LEVEL + baseCurveIndexInBin + localCurveOffset];
    uint invocationIndex = gl_LocalInvocationIndex;
    uint sampleIndex = invocationIndex % sampleCount;

    //float t = 1.2 * (float(segmentIndex) / float(subdivs) - 0.5) + 0.5;
    RationalCubicBezier3DSegment segment = loadProjectedCubicBezierSegment(b_curveData.curves[curveIndex].start);

    float conservative = 8. * sqrt(2.);

    float t = float(sampleIndex) / float(sampleCount - 1);
    vec3 position = evalRationalCubicBezier3D(segment, t);
    vec2 tangent = normalize(evalRationalCubicBezier3DTangent(segment, t).xy);
    vec2 normal = vec2(-tangent.y, tangent.x);
    s_curveSamples[invocationIndex] = CurveSample(position, normal, tangent);
    barrier();

    float hw_aa = strokeWidth + conservative;

    // Emit geometry
    SetMeshOutputsEXT(4 * sampleCount * numCurvesInWorkgroup, 2 * (sampleCount * numCurvesInWorkgroup - 1));


    if (sampleIndex < sampleCount - 1) {
        CurveSample s0 = s_curveSamples[invocationIndex];
        vec2 a = s0.p.xy + s0.n * hw_aa - s0.t * conservative;
        vec2 b = s0.p.xy - s0.n * hw_aa - s0.t * conservative;
        CurveSample s1 = s_curveSamples[invocationIndex+1];
        vec2 c = s1.p.xy + s1.n * hw_aa + s1.t * conservative;
        vec2 d = s1.p.xy - s1.n * hw_aa + s1.t * conservative;
        gl_MeshVerticesEXT[invocationIndex*4 + 0].gl_Position = vec4(windowToNdc(vec3(a, s0.p.z)), 1.0);
        gl_MeshVerticesEXT[invocationIndex*4 + 1].gl_Position = vec4(windowToNdc(vec3(b, s0.p.z)), 1.0);
        gl_MeshVerticesEXT[invocationIndex*4 + 2].gl_Position = vec4(windowToNdc(vec3(c, s1.p.z)), 1.0);
        gl_MeshVerticesEXT[invocationIndex*4 + 3].gl_Position = vec4(windowToNdc(vec3(d, s1.p.z)), 1.0);
        o_curveIndex[invocationIndex*4 + 0] = int(curveIndex);
        o_curveIndex[invocationIndex*4 + 1] = int(curveIndex);
        o_curveIndex[invocationIndex*4 + 2] = int(curveIndex);
        o_curveIndex[invocationIndex*4 + 3] = int(curveIndex);

        vec2 paramRange = b_curveData.curves[curveIndex].paramRange;
        o_uv[invocationIndex*4 + 0] = vec2(remap(t, 0., 1., paramRange.x, paramRange.y), hw_aa);
        o_uv[invocationIndex*4 + 1] = vec2(remap(t, 0., 1., paramRange.x, paramRange.y), -hw_aa);
        o_uv[invocationIndex*4 + 2] = vec2(remap(t, 0., 1., paramRange.x, paramRange.y), hw_aa);
        o_uv[invocationIndex*4 + 3] = vec2(remap(t, 0., 1., paramRange.x, paramRange.y), -hw_aa);

        vec3 cd = b_controlPoints.points[b_curveData.curves[curveIndex].start].color;
        o_color[invocationIndex*4 + 0] = cd;
        o_color[invocationIndex*4 + 1] = cd;
        o_color[invocationIndex*4 + 2] = cd;
        o_color[invocationIndex*4 + 3] = cd;

        vec4 line = vec4(s0.p.xy, s1.p.xy);
        o_line[invocationIndex*2] = line;
        o_line[invocationIndex*2+1] = line;

        uint vtx = invocationIndex*4;
        gl_PrimitiveTriangleIndicesEXT[invocationIndex*2] = uvec3(vtx, vtx+2, vtx+1);
        gl_PrimitiveTriangleIndicesEXT[invocationIndex*2+1] = uvec3(vtx+1, vtx+2, vtx+3);

    } else {
        gl_PrimitiveTriangleIndicesEXT[invocationIndex*2] = uvec3(0, 0, 0);
        gl_PrimitiveTriangleIndicesEXT[invocationIndex*2+1] = uvec3(0, 0, 0);
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

const int MAX_LINES_PER_TILE = 64;

struct TileEntry {
    vec4 line;
    vec2 paramRange;
    uint curveIndex;
};

struct Tile {
    TileEntry[MAX_LINES_PER_TILE] entries;
};

layout(r32i)
uniform coherent iimage2D t_tileLineCount;

buffer TileData {
    Tile tiles[];
} b_tileData;

void main() {
    //uvec2 coord = uvec2(gl_FragCoord.xy);
    //vec2 coordF = vec2(gl_FragCoord.xy);
    //float depth = gl_FragCoord.z;

    ivec2 tilePos = ivec2(gl_FragCoord.xy);
    int tileIndex = tilePos.x + tilePos.y * tilesCountX;
    int count = imageAtomicAdd(t_tileLineCount, tilePos, 1);
    if (count < MAX_LINES_PER_TILE) {
        b_tileData.tiles[tileIndex].entries[count].line = i_line;
        b_tileData.tiles[tileIndex].entries[count].curveIndex = i_curveIndex;
    }

    o_color = vec4(0., 1., 0., 0.2);
}

#endif