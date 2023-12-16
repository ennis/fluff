#version 450 core

layout(set=0,binding=0) uniform texture2D tex;
layout(set=0,binding=1) uniform sampler s;

layout(location=0) in vec2 f_uv;
layout(location=1) in vec4 f_color;

layout(location=0) out vec4 out_color;

void main() {
    out_color = f_color * texture(sampler2D(tex,s), f_uv.st);
}