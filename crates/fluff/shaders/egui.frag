#version 450 core

layout(set=0,binding=0) uniform texture2D u_tex;
layout(set=0,binding=1) uniform sampler u_sampler;

layout(location=0) in vec4 i_color;
layout(location=1) in vec2 i_uv;

layout(location=0) out vec4 o_color;

void main() {
    o_color = i_color * texture(sampler2D(u_tex,u_sampler), i_uv.st);
}