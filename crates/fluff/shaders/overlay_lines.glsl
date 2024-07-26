#version 460 core
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_mesh_shader : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

const uint POLYLINE_START = 1;
const uint POLYLINE_END = 2;
//const uint POLYLINE_SCREEN_SPACE = 4;

//const uint MAX_POLYLINE_VERTICES = 16384;
const uint MESH_WORKGROUP_SIZE = 32;

//////////////////////////////////////////////////////////

struct LineVertex {
    vec3 position;
    u8vec4 color;
    uint flags;
};

// Line vertex buffer
layout(scalar, set=0, binding=0) buffer PositionBuffer {
    LineVertex[] vertices;
};

struct TaskData {
    uint firstVertex;
};

layout(scalar, push_constant) uniform PushConstants {
    mat4 viewProjectionMatrix;
    uint vertexCount;
    float width;
    float filterWidth;
    float screenWidth;
    float screenHeight;
};

//////////////////////////////////////////////////////////

#ifdef __TASK__

layout(local_size_x=MESH_WORKGROUP_SIZE) in;
taskPayloadSharedEXT TaskData taskData;

void main() {
    if (gl_LocalInvocationIndex == 0) {
        taskData.firstVertex = gl_GlobalInvocationID.x;
        EmitMeshTasksEXT(1, 1, 1);
    }
}

#endif

//////////////////////////////////////////////////////////

#ifdef __MESH__

vec4 project(vec3 pos)
{
    vec4 p = vec4(pos, 1.0);
    vec4 clip = viewProjectionMatrix * p;
    return clip;
}


vec3 ndc2win(vec3 ndc)
{
    vec3 window = vec3(ndc.xy * 0.5 + 0.5, ndc.z) * vec3(screenWidth, screenHeight, 1.0);
    return window;
}

vec3 win2ndc(vec3 window)
{
    vec3 ndc = vec3(window.xy / vec2(screenWidth, screenHeight) * 2.0 - 1.0, window.z);
    return ndc;
}

///////////////////////////////////////

taskPayloadSharedEXT TaskData taskData;

shared uint s_lineCount;

layout(location=0) out vec2 o_position[];
layout(location=1) out vec4 o_color[];

layout(local_size_x=MESH_WORKGROUP_SIZE+1) in;  // 33
layout(triangles, max_vertices = (MESH_WORKGROUP_SIZE+1)*2, max_primitives = MESH_WORKGROUP_SIZE*2) out;

void main()
{
    uint baseVtx = taskData.firstVertex;
    // Number of vertices processed by this workgroup
    // This is MESH_WORKGROUP_SIZE for all workgroups except the last one.
    uint groupVtxCount = min(MESH_WORKGROUP_SIZE+1, vertexCount - baseVtx);

    // Vertex processed by this thread (clamp to vertexCount)
    uint vtxIndex = min(baseVtx + gl_LocalInvocationIndex, vertexCount - 1);

    LineVertex vtx = vertices[vtxIndex];
    LineVertex vtx0 = bool(vtx.flags & POLYLINE_START) ? vtx : vertices[vtxIndex - 1];
    LineVertex vtx1 = bool(vtx.flags & POLYLINE_END) ? vtx : vertices[vtxIndex + 1];

    vec4 p = project(vtx.position);
    vec4 p0 = project(vtx0.position);
    vec4 p1 = project(vtx1.position);

    // half-width + anti-aliasing margin
    // clamp the width to 1.0, below that we just fade out the line
    float hw_aa = max(width, 1.0) * 0.5 + filterWidth * sqrt(2.0);

    vec4 a = p;
    vec4 b = p;

    vec2 pxSize = vec2(2. / screenWidth, 2. / screenHeight); // pixel size in clip space

    if (bool(vtx.flags & POLYLINE_START)) {
        vec2 v = p1.xy/p1.w - p.xy/p.w;
        vec2 n = hw_aa * pxSize * normalize(pxSize * vec2(-v.y, v.x));
        a.xy -= n * a.w;
        b.xy += n * b.w;

    } else if (bool(vtx.flags & POLYLINE_END)) {
        vec2 v = p.xy/p.w - p0.xy/p0.w;
        vec2 n = hw_aa * pxSize * normalize(pxSize * vec2(-v.y, v.x));
        a.xy -= n * a.w;
        b.xy += n * b.w;
    }
    else {
        vec2 v0 = normalize((p.xy/p.w - p0.xy/p0.w) / pxSize);
        vec2 v1 = normalize((p1.xy/p1.w - p.xy/p.w) / pxSize);
        vec2 vt = 0.5 * (v0 + v1);
        vec2 n = vec2(-vt.y, vt.x);
        // half-width / sin(theta/2)
        float d = hw_aa / max(cross(vec3(v0, 0.0), vec3(n, 0.0)).z, 0.05);
        // miter points
        a.xy -= d * n * pxSize * a.w;
        b.xy += d * n * pxSize * b.w;

        /*// NOTE: this may go outside the fragment, but that's OK
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
        b = p.xy + d * n;*/
    }

    //if (enabled) {
    gl_MeshVerticesEXT[gl_LocalInvocationIndex*2+0].gl_Position = a;
    gl_MeshVerticesEXT[gl_LocalInvocationIndex*2+1].gl_Position = b;
    o_color[gl_LocalInvocationIndex*2+0] = vec4(vtx.color) / 255.0;
    o_color[gl_LocalInvocationIndex*2+1] = vec4(vtx.color) / 255.0;
    o_position[gl_LocalInvocationIndex*2+0] = vec2(0.0, -hw_aa);
    o_position[gl_LocalInvocationIndex*2+1] = vec2(1.0, hw_aa);
    //}

    //barrier();

    if (gl_LocalInvocationIndex == 0) {
        SetMeshOutputsEXT(groupVtxCount * 2, (groupVtxCount - 1) * 2);
    }
    else {
        // emit 2 triangles
        // A--C
        // | /|
        // |/ |
        // B--D
        // ACB, BCD
        uint i = (gl_LocalInvocationIndex - 1) * 2;
        gl_PrimitiveTriangleIndicesEXT[i] = uvec3(i, i+2, i+1);
        gl_PrimitiveTriangleIndicesEXT[i+1] = uvec3(i+1, i+2, i+3);
        bool cull = (vtx.flags & POLYLINE_START) != 0;
        gl_MeshPrimitivesEXT[i].gl_CullPrimitiveEXT = cull;
        gl_MeshPrimitivesEXT[i+1].gl_CullPrimitiveEXT = cull;
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