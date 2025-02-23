import "shared.slang";

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

struct PushConstants {
    float4x4 matrix;
    float width;
};

[shader("vertex")]
VertexOutput overlayPolygonsVertexMain(Vertex v, uniform PushConstants u) {
    VertexOutput o;
    o.pos = mul(u.matrix, float4(v.pos, 1.0));
    o.color = v.col;
    return o;
}

[shader("fragment")]
FragmentOutput overlayPolygonsFragmentMain(VertexOutput v, uniform PushConstants u) {
    FragmentOutput o;
    o.color = v.color;
    return o;
}