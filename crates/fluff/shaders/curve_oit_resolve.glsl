#version 460 core
#include "common.glsl"

#extension GL_EXT_scalar_block_layout : require

///////////////////////////////////////////

#ifdef __VERTEX__
void main() {
    vec2 uv = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position = vec4(uv * 2.0 + -1.0, 0.0, 1.0);
}
#endif // __VERTEX__

const int MAX_FRAGMENTS_PER_PIXEL = 8;

///////////////////////////////////////////

#ifdef __FRAGMENT__

// Per-fixel fragment data before sorting and blending
struct FragmentData {
    vec4 color;
    float depth;
};

// Fragment buffer
layout(set=0,binding=0) buffer FragmentBuffer {
    FragmentData[] fragments;
};

layout(set=0,binding=1) buffer FragmentCountBuffer {
    uint[] fragmentCount;
};

layout(push_constant, scalar) uniform PushConstants {
    uvec2 viewportSize;
};


layout(location=0) out vec4 o_color;

void main() {
    uvec2 coord = uvec2(gl_FragCoord.xy);

    uint pixelIndex = coord.y * viewportSize.x + coord.x;
    uint fragCount = fragmentCount[pixelIndex];
    uint base = pixelIndex * MAX_FRAGMENTS_PER_PIXEL;

    vec4 color = vec4(0.0);
    for (int i = int(fragCount) - 1; i >= 0; i--) {
        vec4 fragColor = fragments[base + i].color;
        color.rgb = (1.0 - fragColor.a) * color.rgb + fragColor.rgb;
        color.a = (1.0 - fragColor.a) * color.a + fragColor.a;
    }

    color.rgb /= max(color.a,0.01);
    o_color = color;
}

#endif // __FRAGMENT__
