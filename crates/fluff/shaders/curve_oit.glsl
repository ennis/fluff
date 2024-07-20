#version 460 core
#include "common.inc.glsl"

#extension GL_EXT_mesh_shader : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_ARB_fragment_shader_interlock : require

//#define USE_OIT

// Questions:
// 1. evaluate the curve analytically VS discretize to polyline
// 2. render directly VS render curve IDs to buckets and then evaluate per-bucket
// 3. curve width profiles (assume constant width for now)
// 4. curve color profiles (use curve parametrization & color ramp)
// 5. transparency (which OIT method)

// (2.) We can't share a lot of work among pixels in a bucket: sorting will have to be done per-pixel anyway


// Tentative pipeline:
// - Rasterize the curve skeleton directly, inflate with a mesh shader
// - Evaluate the curve analytically in a fragment shader, blend samples using rasterizer order views


const int SAMPLE_COUNT = 16;
// Number of subdivisions per curve segment
const int SUBDIV_COUNT = SAMPLE_COUNT - 1;
const int MAX_FRAGMENTS_PER_PIXEL = 8;

//////////////////////////////////////////////////////////

// A range of control points in the controlPoints buffer that describes a single curve.
struct CurveDescriptor {
    vec4 widthProfile;// polynomial coefficients
    vec4 opacityProfile;// polynomial coefficients
    uint start;
    uint size;
    uvec2 padding;
};

// Per-fixel fragment data before sorting and blending
struct FragmentData {
    vec4 color;
    float depth;
};

struct ControlPoint {
    vec3 pos;
    vec3 color;
};

//////////////////////////////////////////////////////////

layout(set=0, binding=0, scalar) buffer PositionBuffer {
    ControlPoint controlPoints[];
};

layout(set=0, binding=1, scalar) buffer CurveBuffer {
    CurveDescriptor curves[];
};

// Push constants
layout(push_constant, scalar) uniform PushConstants {
    mat4 viewProjectionMatrix;
    uvec2 viewportSize;
    float strokeWidth;
    int baseCurveIndex;
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
RationalCubicBezier3DSegment loadProjectedCubicBezierSegment(uint baseControlPoint, uint segmentIndex) {
    vec3 p0 = controlPoints[baseControlPoint + segmentIndex*3 + 0].pos;
    vec3 p1 = controlPoints[baseControlPoint + segmentIndex*3 + 1].pos;
    vec3 p2 = controlPoints[baseControlPoint + segmentIndex*3 + 2].pos;
    vec3 p3 = controlPoints[baseControlPoint + segmentIndex*3 + 3].pos;

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

// Calculates the closest point on a bezier curve to a given point in screen space
void distCubicBezier3DProj(RationalCubicBezier3DSegment segment, vec2 pos, out vec3 closestPoint, out float t, out float closestDist) {
    // initial search
    const int maxStep = 16;// max step for the initial search
    float minDist = 99999999.;// current minimum distance
    int index = 0;// index of the closest point
    for (int i = 0; i <= maxStep; ++i) {
        float d = distance(pos, evalRationalCubicBezier3D(segment, float(i) / maxStep).xy);
        if (d < minDist) {
            index = i;
            minDist = d;
        }
    }

    float t_prev = float(max(index - 1, 0))  / maxStep;
    float t_next = float(min(index + 1, maxStep)) / maxStep;
    float t_mid = float(index) / maxStep;

    PositionAndParam points[3] = PositionAndParam[3](
    PositionAndParam(evalRationalCubicBezier3D(segment, t_prev), t_prev),
    PositionAndParam(evalRationalCubicBezier3D(segment, t_mid), t_mid),
    PositionAndParam(evalRationalCubicBezier3D(segment, t_next), t_next));

    // refinement
    const int numRefineSteps = 6;
    for (int i = 0; i < numRefineSteps; ++i) {
        PositionAndParam prev = points[0];
        PositionAndParam next = points[2];
        PositionAndParam mid = points[1];

        vec3 mid_prev = evalRationalCubicBezier3D(segment, .5 * (prev.t + mid.t));// halfway bewteen prev and mid
        vec3 mid_next = evalRationalCubicBezier3D(segment, .5 * (next.t + mid.t));// halfway between next and mid

        float d_prev = distance(pos, mid_prev.xy);
        float d_next = distance(pos, mid_next.xy);
        float d_mid = distance(pos, mid.pos.xy);

        // find the closest point between the three
        float md = 99999999.;
        int closest = 0;
        if (d_prev < md) {
            md = d_prev;
            closest = 0;
        }
        if (d_mid < md) {
            md = d_mid;
            closest = 1;
        }
        if (d_next < md) {
            closest = 2;
        }

        if (closest == 0) {
            points[2] = mid;
            points[1] = PositionAndParam(mid_prev, .5 * (prev.t + mid.t));
        } else if (closest == 1) {
            points[0] = PositionAndParam(mid_prev, .5 * (prev.t + mid.t));
            points[2] = PositionAndParam(mid_next, .5 * (next.t + mid.t));
        } else {
            points[0] = mid;
            points[1] = PositionAndParam(mid_next, .5 * (next.t + mid.t));
        }
    }

    closestPoint = points[1].pos;
    t = points[1].t;
    closestDist = distance(pos, closestPoint.xy);
}

//////////////////////////////////////////////////////////

#ifdef __MESH__

// One thread per sample on the curve
layout(local_size_x=SAMPLE_COUNT) in;

// Output: triangle mesh tessellation of the curve
// 2 vertices per sample, 2 triangles subdivision
layout(triangles, max_vertices=SAMPLE_COUNT*2, max_primitives=SUBDIV_COUNT*2) out;
layout(location=0) flat out int o_curveIndex[];

struct CurveSample {
    vec3 p;
    vec2 n;
};
shared CurveSample[SAMPLE_COUNT] s_curveSamples;

void main()
{
    // One work group per curve segment, one thread per sample on the curve
    uint curveIndex = baseCurveIndex + gl_WorkGroupID.x;
    uint sampleIndex = gl_LocalInvocationID.x;

    // TODO handle curves with more than one segment
    RationalCubicBezier3DSegment curveSegment = loadProjectedCubicBezierSegment(curves[curveIndex].start, 0);
    float t = 1.2 * (float(sampleIndex) / float(SAMPLE_COUNT - 1) - 0.5) + 0.5;

    vec3 position = evalRationalCubicBezier3D(curveSegment, t);
    vec2 tangent = normalize(evalRationalCubicBezier3DTangent(curveSegment, t).xy);
    vec2 normal = vec2(-tangent.y, tangent.x);

    float hw_aa = strokeWidth * sqrt(2);

    s_curveSamples[sampleIndex] = CurveSample(position, normal);
    barrier();

    if (sampleIndex == 0) {
        SetMeshOutputsEXT(SAMPLE_COUNT*2, SUBDIV_COUNT*2);
        CurveSample s0 = s_curveSamples[sampleIndex];
        vec2 a = s0.p.xy + s0.n * hw_aa;
        vec2 b = s0.p.xy - s0.n * hw_aa;
        gl_MeshVerticesEXT[0].gl_Position = vec4(windowToNdc(vec3(a, s0.p.z)), 1.0);
        gl_MeshVerticesEXT[1].gl_Position = vec4(windowToNdc(vec3(b, s0.p.z)), 1.0);
        o_curveIndex[0] = int(curveIndex);
        o_curveIndex[1] = int(curveIndex);
    }

    // Emit triangles for each subdivision
    if (sampleIndex > 0) {
        CurveSample s = s_curveSamples[sampleIndex];
        vec2 c = s.p.xy + s.n * hw_aa;
        vec2 d = s.p.xy - s.n * hw_aa;
        gl_MeshVerticesEXT[sampleIndex*2 + 0].gl_Position = vec4(windowToNdc(vec3(c, s.p.z)), 1.0);
        gl_MeshVerticesEXT[sampleIndex*2 + 1].gl_Position = vec4(windowToNdc(vec3(d, s.p.z)), 1.0);
        o_curveIndex[sampleIndex*2 + 0] = int(curveIndex);
        o_curveIndex[sampleIndex*2 + 1] = int(curveIndex);

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

#endif// __MESH__

//////////////////////////////////////////////////////////

#ifdef __FRAGMENT__

// Use "ordered" for additional temporal coherence
#ifdef USE_OIT
layout(pixel_interlock_ordered) in;
#endif

layout(location=0) flat in int i_curveIndex;
layout(location=0) out vec4 o_color;

// Fragment buffer
layout(set=0, binding=2) coherent buffer FragmentBuffer {
    FragmentData[] fragments;
};

layout(set=0, binding=3) coherent buffer FragmentCountBuffer {
    uint[] fragmentCount;
};

float evalPolynomial(vec4 coeffs, float x) {
    float x2 = x * x;
    float x3 = x2 * x;
    return clamp(dot(coeffs, vec4(1.0, x, x2, x3)), 0.0, 1.0);
}

void main() {
    uvec2 coord = uvec2(gl_FragCoord.xy);
    vec2 coordF = vec2(gl_FragCoord.xy);
    float depth = gl_FragCoord.z;

    RationalCubicBezier3DSegment curveSegment = loadProjectedCubicBezierSegment(curves[i_curveIndex].start, 0);

    // Evaluate curve DF
    vec3 closestPoint;
    float t;
    float dist;
    distCubicBezier3DProj(curveSegment, coordF, closestPoint, t, dist);

    // Evaluate curve profile functions at t (width, opacity)
    float width = strokeWidth * evalPolynomial(curves[i_curveIndex].widthProfile, t);
    float opacity = evalPolynomial(curves[i_curveIndex].opacityProfile, t);

    // Compute color
    float mask = 1.0-smoothstep(width-1., width+1., dist);
    float outerStrokeMask = 1.0 - smoothstep(1.0, 2.0, abs(dist - width));
    vec3 color = controlPoints[curves[i_curveIndex].start].color;
    /* palette(1.0-t,
            vec3(0.358,0.588,-1.032),
            vec3(0.428,0.288,0.600),
            vec3(0.538,0.228,-0.312),
            vec3(0.558,0.000,0.667));*/
    //color = mix(color, vec3(0.0,0.0,0.0), outerStrokeMask);
    FragmentData frag = FragmentData(mask * opacity * vec4(color, 1.0), depth);

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


    //o_color = vec4(0.0,0.0,0.0,0.2);
    o_color = vec4(frag.color.rgb / frag.color.a, frag.color.a);
    //o_color = vec4(color, mask);
}

#endif