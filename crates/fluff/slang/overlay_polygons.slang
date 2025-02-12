import "bindless.slang";

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

//  Maximum number of line segments per tile.
static const uint MAX_LINES_PER_TILE = 32;

struct TileLineData {
    float4 lineCoords;
    float2 paramRange;
    uint curveId;
    float depth;
};

struct TileData {
    TileLineData[MAX_LINES_PER_TILE] lines;
};


//  3D bezier control point.
struct ControlPoint {
    float3 pos;
    float3 color;
};

//  Scene camera parameters.
struct SceneParams {
    float4x4 view;
    float4x4 proj;
    float4x4 viewProj;
    float3 eye;
    float nearClip;
    float farClip;
    float left;
    float right;
    float top;
    float bottom;
    uint2 viewportSize;
    float2 cursorPos;
    float time;
};


struct ComputeTestParams {
    uint elementCount;
    TileData* data;
    ControlPoint* controlPoints;
    RWTexture2D<float4>.Handle outputImage;
};


//  Represents a range of control points in the position buffer.
struct CurveDesc {
    float4 widthProfile;
    float4 opacityProfile;
    uint start;
    uint count;
    float2 paramRange;
    uint brushIndex;
};

struct DrawCurvesPushConstants {
    ControlPoint* controlPoints;
    CurveDesc* curves;
    SceneParams* sceneParams;
    uint baseCurveIndex;
    float strokeWidth;
    uint tileCountX;
    uint tileCountY;
    uint frame;
    TileData* tileData;
    uint* tileLineCount;
    Texture2D.Handle* brushTextures;
    RWTexture2D<float4>.Handle outputImage;
    SamplerState.Handle brushTextureSampler;
    uint debugOverflow;
    float strokeBleedExp;
};


struct PushConstants {
    float4x4 matrix;
    float width;
    float* pointer;
};

struct Vertex {
    [[vk::location(0)]] float3 pos;
    [[vk::location(1)]] float4 col;
};

struct VertexOutput {
    float4 pos: SV_Position;
    [[vk::location(0)]] float4 color;
};

struct FragmentOutput
{
    [[vk::location(0)]] float4 color;
};

[shader("vertex")]
VertexOutput vtxMain(Vertex v, uniform PushConstants u) {
    VertexOutput o;
    o.pos = mul(u.matrix, float4(v.pos, 1.0));
    o.color = v.col;
    return o;
}

[shader("fragment")]
FragmentOutput dragCurvesFragMain(VertexOutput v, uniform DrawCurvesPushConstants u) {
    FragmentOutput o;
    o.color = v.color + u.brushTextures[0].Sample(u.brushTextureSampler, v.pos.xy);
    return o;
}