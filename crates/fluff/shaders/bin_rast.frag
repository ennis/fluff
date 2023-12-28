#version 460 core

#extension GL_EXT_shader_atomic_float : require

const int MAX_CURVES_PER_TILE = 16;

layout(set=0,binding=2,r32i) uniform coherent iimage2D tileCurveCountImage;

struct Tile {
    uint curves[MAX_CURVES_PER_TILE];
};

layout(std430,set=0,binding=3) buffer CurveBuffer {
    Tile tiles[];
};

layout(std140, push_constant) uniform PushConstants {
    mat4 viewProjectionMatrix;
    int baseCurve;
    float strokeWidth;
    int tilesCountX;
    int tilesCountY;
};

layout(location=0) flat in int curveIndex;
layout(location=0) out vec4 color;

void main() {
    ivec3 tilePos = ivec3(gl_FragCoord);
    int tileIndex = tilePos.x + tilePos.y * tilesCountX;

    int count = imageAtomicAdd(tileCurveCountImage, tilePos.xy, 1);
    if (count < MAX_CURVES_PER_TILE) {
        tiles[tileIndex].curves[count] = curveIndex;
    }
    color = vec4(0., 1., 0., 0.2);
}