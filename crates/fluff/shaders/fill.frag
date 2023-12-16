#version 450
#extension GL_NV_uniform_buffer_std430_layout : enable

#pragma include <common.glsl>


layout(location=0) in vec2 position;
layout(location=0) out vec4 out_color;

void main() {
    vec2 uv = 0.5 * vec2(position.x, position.y) + 0.5;
   // vec2 resolution = vec2(screenWidth, screenHeight);
   // vec2 screenPos = uv * resolution;

    out_color = vec4(0.0, 1.0, 0.0, 1.0);
}
