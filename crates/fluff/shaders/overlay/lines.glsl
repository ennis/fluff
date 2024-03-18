#version 460 core
#extension GL_EXT_mesh_shader : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

#include "../common.glsl"

// A group of at most 64 points in a polyline.
struct PolylineFragment {
    uint startVertex;
    uint vertexCount;// max 64
    uint flags;// POLYLINE_START, POLYLINE_END
};

struct TaskData {
    uint fragmentCount;
    PolylineFragment[1024] fragments;
};

struct Polyline {
    // Start index in the vertex buffer. (in number of vec3s)
    uint startVertex;
    // Number of points (vec3). The number of line segments is count-1.
    //
    // The maximum number of points is MAX_POLYLINE_POINTS.
    uint vertexCount;
};

const uint POLYLINE_START = 1;
const uint POLYLINE_END = 2;

const uint MAX_POLYLINE_VERTICES = 16384;
const uint POLYLINE_FRAGMENT_VERTICES = 32;

//////////////////////////////////////////////////////////

struct LineVertex {
    float[3] position;
    u8vec4 color;
};

// Line vertex buffer
layout(std430, set=0, binding=0) buffer PositionBuffer {
    LineVertex[] vertices;
};

layout(std430, set=0, binding=1) buffer LineBuffer {
    Polyline[] polylines;
};

layout(std140, push_constant) uniform PushConstants {
    mat4 viewProjectionMatrix;
    uint lineCount;
    float width;
    float filterWidth;
    float screenWidth;
    float screenHeight;
};

//////////////////////////////////////////////////////////

#ifdef __TASK__

layout(local_size_x=32) in;
taskPayloadSharedEXT TaskData taskData;

void main() {

    // process one polyline per thread
    uint index = gl_GlobalInvocationID.x;

    if (index >= lineCount) {
        return;
    }

    if (gl_LocalInvocationIndex == 0) {
        taskData.fragmentCount = 0;
    }

    // split the polyline into smaller polylines of POLYLINE_FRAGMENT_VERTICES vertices
    uint start = polylines[index].startVertex;
    uint count = polylines[index].vertexCount;

    uint numFragments = (count + POLYLINE_FRAGMENT_VERTICES - 1) / POLYLINE_FRAGMENT_VERTICES;
    uint pos = atomicAdd(taskData.fragmentCount, numFragments);

    for (uint i = 0; i < numFragments; i++) {
        uint fragmentStartVertex = start + i * POLYLINE_FRAGMENT_VERTICES;
        uint fragmentVertexCount = min(POLYLINE_FRAGMENT_VERTICES, (start + count) - fragmentStartVertex);
        uint flags = 0;
        if (i == 0) {
            flags |= POLYLINE_START;
        }
        if (i == numFragments - 1) {
            flags |= POLYLINE_END;
        }
        taskData.fragments[pos + i] = PolylineFragment(fragmentStartVertex, fragmentVertexCount, flags);
    }

    if (gl_LocalInvocationIndex == 0) {
        EmitMeshTasksEXT(taskData.fragmentCount, 1, 1);
    }
}

#endif

//////////////////////////////////////////////////////////

#ifdef __MESH__

taskPayloadSharedEXT TaskData taskData;

layout(local_size_x=POLYLINE_FRAGMENT_VERTICES) in;
layout(triangles, max_vertices = POLYLINE_FRAGMENT_VERTICES*2, max_primitives = POLYLINE_FRAGMENT_VERTICES*2) out;

layout(location=0) out vec2 o_position[];
layout(location=1) out vec4 o_color[];

vec4 project(vec3 pos, out bool clipped)
{
    vec4 p = vec4(pos, 1.0);
    vec4 clip = viewProjectionMatrix * p;
    clipped = clip.w <= 0.0;
    vec3 ndc = clip.xyz / clip.w;
    vec3 window = vec3(ndc.xy * 0.5 + 0.5, ndc.z) * vec3(screenWidth, screenHeight, 1.0);
    return vec4(window, clip.w);
}

vec3 fetchPosition(uint index)
{
    float[3] p = vertices[index].position;
    return vec3(p[0], p[1], p[2]);
}

vec3 ndcToWindow(vec3 ndc)
{
    vec3 window = vec3(ndc.xy * 0.5 + 0.5, ndc.z) * vec3(screenWidth, screenHeight, 1.0);
    return window;
}

vec3 windowToNdc(vec3 window)
{
    vec3 ndc = vec3(window.xy / vec2(screenWidth, screenHeight) * 2.0 - 1.0, window.z);
    return ndc;
}

void main()
{
    uint fragmentIndex = gl_WorkGroupID.x;
    uint vertexIndex = gl_LocalInvocationID.x;

    PolylineFragment fragment = taskData.fragments[fragmentIndex];
    if (vertexIndex >= fragment.vertexCount) {
        return;
    }

    bool clipped = false;
    bool cc = false;
    vec4 pp = project(fetchPosition(fragment.startVertex + vertexIndex), clipped);
    vec3 p = pp.xyz;
    float w = pp.w;
    u8vec4 color = vertices[fragment.startVertex + vertexIndex].color;

    // half-width + anti-aliasing margin
    // clamp the width to 1.0, below that we just fade out the line

    float hw_aa = max(width, 1.0) * 0.5 + filterWidth * sqrt(2.0);

    vec2 a;
    vec2 b;

    if (bool(fragment.flags & POLYLINE_START) && vertexIndex == 0) {
        vec3 p1 = project(fetchPosition(fragment.startVertex + vertexIndex + 1), cc).xyz;
        vec2 v = normalize(p1.xy - p.xy);
        vec2 n = vec2(-v.y, v.x);
        a = p.xy - hw_aa * n;
        b = p.xy + hw_aa * n;
    } else if (bool(fragment.flags & POLYLINE_END) && vertexIndex == fragment.vertexCount - 1) {
        vec3 p0 = project(fetchPosition(fragment.startVertex + vertexIndex - 1), cc).xyz;
        vec2 v = normalize(p.xy - p0.xy);
        vec2 n = vec2(-v.y, v.x) ;
        a = p.xy - hw_aa * n;
        b = p.xy + hw_aa * n;
    }
    else {
        // NOTE: this may go outside the fragment, but that's OK
        vec3 p0 = project(fetchPosition(fragment.startVertex + vertexIndex - 1), cc).xyz;
        vec3 p1 = project(fetchPosition(fragment.startVertex + vertexIndex + 1), cc).xyz;
        vec2 v0 = normalize(p.xy - p0.xy);
        vec2 v1 = normalize(p1.xy - p.xy);
        vec2 vt = 0.5 * (v0 + v1);
        vec2 n = vec2(-vt.y, vt.x);
        // half-width / sin(theta/2)
        float d = hw_aa / max(cross(vec3(v0, 0.0), vec3(n, 0.0)).z, 0.05);
        // miter points
        a = p.xy - d * n;
        b = p.xy + d * n;
    }

    gl_MeshVerticesEXT[vertexIndex*2+0].gl_Position = vec4(windowToNdc(vec3(a, p.z)), clipped ? 0.0 : 1.0);
    gl_MeshVerticesEXT[vertexIndex*2+1].gl_Position = vec4(windowToNdc(vec3(b, p.z)), clipped ? 0.0 : 1.0);
    o_color[vertexIndex*2+0] = vec4(color) / 255.0;
    o_color[vertexIndex*2+1] = vec4(color) / 255.0;
    o_position[vertexIndex*2+0] = vec2(0.0, -hw_aa);
    o_position[vertexIndex*2+1] = vec2(1.0, hw_aa);

    if (vertexIndex == 0) {
        SetMeshOutputsEXT(fragment.vertexCount * 2, (fragment.vertexCount - 1) * 2);
    }
    else {
        // emit 2 triangles
        // A--C
        // | /|
        // |/ |
        // B--D
        // ACB, BCD
        uint vtx = (vertexIndex - 1) * 2;
        gl_PrimitiveTriangleIndicesEXT[(vertexIndex-1)*2] = uvec3(vtx, vtx+2, vtx+1);
        gl_PrimitiveTriangleIndicesEXT[(vertexIndex-1)*2+1] = uvec3(vtx+1, vtx+2, vtx+3);
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