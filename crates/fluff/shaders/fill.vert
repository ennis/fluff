#version 450

uniform vec2 screenSize;

layout(location=0) in vec2 a_position;
layout(location=0) out vec2 v_position;

void main() {
    gl_Position = vec4(a_position, 0.0, 1.0);
    v_position = a_position;
}