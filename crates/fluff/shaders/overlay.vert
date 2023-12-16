#version 450 core

layout(std140, push_constant) uniform PushConstants {
    mat4 matrix;
};

layout(location=0) in vec3 pos;
layout(location=1) in vec4 col;

layout(location=0) out vec4 f_color;

void main() {
    gl_Position = matrix * vec4(pos, 1.0);
    f_color = col;
}

