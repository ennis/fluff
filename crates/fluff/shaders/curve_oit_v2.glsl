#version 460 core
#include "common.glsl"

#extension GL_EXT_mesh_shader : require
#extension GL_ARB_fragment_shader_interlock : require

//#define USE_OIT

const int MAX_FRAGMENTS_PER_PIXEL = 8;

//////////////////////////////////////////////////////////

// A range of control points in the controlPoints buffer that describes a single curve.
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

layout(set=0,binding=0,scalar) buffer PositionBuffer {
    ControlPoint controlPoints[];
};

layout(set=0,binding=1,scalar) buffer CurveBuffer {
    CurveDescriptor curves[];
};

// Push constants
layout(push_constant, scalar) uniform PushConstants {
    mat4 viewProjectionMatrix;
    uvec2 viewportSize;
    float strokeWidth;
    int baseCurveIndex;
    int curveCount;
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
    vec3 p0 = controlPoints[baseControlPoint + 0].pos;
    vec3 p1 = controlPoints[baseControlPoint + 1].pos;
    vec3 p2 = controlPoints[baseControlPoint + 2].pos;
    vec3 p3 = controlPoints[baseControlPoint + 3].pos;

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


    //vec3 p0 = ndcToWindow(project(controlPoints[baseControlPoint + segmentIndex*3 + 0]));
    //vec3 p1 = ndcToWindow(project(controlPoints[baseControlPoint + segmentIndex*3 + 1]));
    //vec3 p2 = ndcToWindow(project(controlPoints[baseControlPoint + segmentIndex*3 + 2]));
    //vec3 p3 = ndcToWindow(project(controlPoints[baseControlPoint + segmentIndex*3 + 3]));
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
const int MAX_VERTICES_PER_CURVE = 128;
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
    RationalCubicBezier3DSegment segment = loadProjectedCubicBezierSegment(curves[curveIndex].start);

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
        uint numWorkgroupsToLaunchForBucket = divCeil(taskData.subdivisionBinSizes[i], curvesPerWorkgroup);
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
layout(triangles, max_vertices=2*MAX_VERTICES_PER_CURVE, max_primitives=2*MAX_VERTICES_PER_CURVE) out;
layout(location=0) flat out int o_curveIndex[];
layout(location=1) out vec2 uv[];
layout(location=2) out vec3 color[];

struct CurveSample {
    vec3 p;
    vec2 n;
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

    uint sampleCount = subdivisionPointCount(subdivisionLevel);
    uint localCurveOffset = gl_LocalInvocationIndex / sampleCount;
    uint curveIndex = taskData.subdivisionBins[subdivisionLevel*MAX_CURVES_PER_SUBDIVISION_LEVEL + baseCurveIndexInBin + localCurveOffset];
    uint sampleIndex = gl_LocalInvocationIndex;

    //float t = 1.2 * (float(segmentIndex) / float(subdivs) - 0.5) + 0.5;
    RationalCubicBezier3DSegment segment = loadProjectedCubicBezierSegment(curves[curveIndex].start);

    float t = float(sampleIndex % sampleCount) / float(sampleCount - 1);
    vec3 position = evalRationalCubicBezier3D(segment, t);
    vec2 tangent = normalize(evalRationalCubicBezier3DTangent(segment, t).xy);
    vec2 normal = vec2(-tangent.y, tangent.x);
    s_curveSamples[sampleIndex] = CurveSample(position, normal);
    barrier();

    float hw_aa = strokeWidth * sqrt(2);

    // Emit geometry
    SetMeshOutputsEXT(2 * sampleCount * numCurvesInWorkgroup, 2 * (sampleCount * numCurvesInWorkgroup - 1));

    // One vertex per sample
    CurveSample s = s_curveSamples[sampleIndex];
    vec2 a = s.p.xy + s.n * hw_aa;
    vec2 b = s.p.xy - s.n * hw_aa;
    gl_MeshVerticesEXT[sampleIndex*2 + 0].gl_Position = vec4(windowToNdc(vec3(a, s.p.z)), 1.0);
    gl_MeshVerticesEXT[sampleIndex*2 + 1].gl_Position = vec4(windowToNdc(vec3(b, s.p.z)), 1.0);
    o_curveIndex[sampleIndex*2 + 0] = int(curveIndex);
    o_curveIndex[sampleIndex*2 + 1] = int(curveIndex);

    vec2 paramRange = curves[curveIndex].paramRange;
    uv[sampleIndex*2 + 0] = vec2(remap(t, 0., 1., paramRange.x, paramRange.y), hw_aa);
    uv[sampleIndex*2 + 1] = vec2(remap(t, 0., 1., paramRange.x, paramRange.y), -hw_aa);

    vec3 cd = controlPoints[curves[curveIndex].start].color;
    color[sampleIndex*2 + 0] = cd;
    color[sampleIndex*2 + 1] = cd;

    // Two triangles per sample, except at the beginning of curves
    uint vtx = (sampleIndex - 1) * 2;
    if (sampleIndex % sampleCount == 0) {
        if (sampleIndex > 0) {
            gl_PrimitiveTriangleIndicesEXT[(sampleIndex-1)*2] = uvec3(0, 0, 0);
            gl_PrimitiveTriangleIndicesEXT[(sampleIndex-1)*2+1] = uvec3(0, 0, 0);
        }
    } else {
        // emit 2 triangles
        // A--C
        // | /|
        // |/ |
        // B--D
        // ACB, BCD
        uint vtx = (sampleIndex - 1) * 2;
        gl_PrimitiveTriangleIndicesEXT[(sampleIndex-1)*2] = uvec3(vtx, vtx+2, vtx+1);
        gl_PrimitiveTriangleIndicesEXT[(sampleIndex-1)*2+1] = uvec3(vtx+1, vtx+2, vtx+3);
    }
}

#endif  // __MESH__

//////////////////////////////////////////////////////////

#ifdef __FRAGMENT__

// Use "ordered" for additional temporal coherence
#ifdef USE_OIT
layout(pixel_interlock_ordered) in;
#endif

layout(location=0) flat in int i_curveIndex;
layout(location=1) in vec2 i_uv;
layout(location=2) in vec3 i_color;
layout(location=0) out vec4 o_color;

// Fragment buffer
layout(set=0,binding=2) coherent buffer FragmentBuffer {
    FragmentData[] fragments;
};

layout(set=0,binding=3) coherent buffer FragmentCountBuffer {
    uint[] fragmentCount;
};

layout(set=0,binding=4) uniform texture2D u_tex;
layout(set=0,binding=5) uniform sampler u_sampler;


float evalPolynomial(vec4 coeffs, float x) {
    float x2 = x * x;
    float x3 = x2 * x;
    return clamp(dot(coeffs, vec4(1.0, x, x2, x3)), 0.0, 1.0);
}

void main() {
    uvec2 coord = uvec2(gl_FragCoord.xy);
    vec2 coordF = vec2(gl_FragCoord.xy);
    float depth = gl_FragCoord.z;

    //RationalCubicBezier3DSegment curveSegment = loadProjectedCubicBezierSegment(curves[i_curveIndex].start);

    float t = i_uv.x;
    float dist = abs(i_uv.y);
    // Evaluate curve profile functions at t (width, opacity)
    float width = strokeWidth * evalPolynomial(curves[i_curveIndex].widthProfile, t);
    float opacity = 0.5*pow(evalPolynomial(curves[i_curveIndex].opacityProfile, t), 6.0);

    // Compute color

    //float brushMask = 1.0 - texture(sampler2D(u_tex, u_sampler), vec2(i_uv.x * 10., i_uv.y / strokeWidth * .5 + .5)).r;

    float mask = 1.-linearstep(width-1., width+1., dist);
    float outerStrokeMask = 1.0 - smoothstep(1.0, 1.2, abs(dist - width));
    /*vec3 color = palette(1.0-t,
            vec3(0.358,0.588,-1.032),
            vec3(0.428,0.288,0.600),
            vec3(0.538,0.228,-0.312),
            vec3(0.558,0.000,0.667));*/
    //vec4 color = mix(color, vec3(0.0,0.0,0.0), outerStrokeMask);
    FragmentData frag = FragmentData(mask * opacity * vec4(i_color, 1.0), depth);

#ifdef USE_OIT
    // Insert in fragment buffer
    beginInvocationInterlockARB();
    uint pixelIndex = coord.y * viewportSize.x + coord.x;
    uint fragCount = fragmentCount[pixelIndex];
    uint index = pixelIndex * MAX_FRAGMENTS_PER_PIXEL;

    for (uint i = 0; i < fragCount; i++) {
        if (frag.depth < fragments[index].depth) {
            FragmentData temp = frag;
            frag = fragments[index];
            fragments[index] = temp;
        }
        index++;
    }

    if (fragCount < MAX_FRAGMENTS_PER_PIXEL) {
        fragments[pixelIndex * MAX_FRAGMENTS_PER_PIXEL + fragCount] = frag;
        fragmentCount[pixelIndex] = fragCount + 1;
    }
    memoryBarrierBuffer();
    endInvocationInterlockARB();
#else
    //beginInvocationInterlockARB();
    //uint pixelIndex = coord.y * viewportSize.x + coord.x;
    //uint fragCount = fragmentCount[pixelIndex];
    //fragments[pixelIndex * MAX_FRAGMENTS_PER_PIXEL] = frag;
    //fragmentCount[pixelIndex] = 1;
    //memoryBarrierBuffer();
    //endInvocationInterlockARB();
#endif

    // stochastic transparency test
    if (.5+.5*hash(ivec2(coord.xy) + ivec2(48*(frame+i_curveIndex))) > frag.color.a) {
        discard;
    }


    //o_color = vec4(1.0,0.0,0.0,1.0);
    o_color = vec4(i_color, 1.0);
    //o_color = vec4(color, mask);
}

#endif