#version 460 core
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_shader_explicit_arithmetic_types : require

#include "common.inc.glsl"
#include "bindless.inc.glsl"
#include "shared.inc.glsl"

//////////////////////////////////////////////////////////
layout(scalar, push_constant) uniform PushConstants {
    VisualizerPushConstants u;
};

//////////////////////////////////////////////////////////

#ifdef __VERTEX__

void main() {
    vec4 value = texelFetch(u.texture, ivec2(u.sceneParams.d.cursorPos), 0);

    // interpret as screen-space normal
}

#endif

//////////////////////////////////////////////////////////

#ifdef __FRAGMENT__

layout(location=0) in vec2 i_position;
layout(location=1) in vec4 i_color;
layout(location=0) out vec4 o_color;

void main() {

}

#endif