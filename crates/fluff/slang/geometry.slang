static const uint POSITION_ATTR = 0;
static const uint NORMAL_ATTR = 1;
static const uint TEXCOORD_ATTR = 2;
static const uint COLOR_ATTR = 3;


struct VertexOutput {
    float4 position : SV_Position;
    [[vk::location(0)]] float3 normal;
    [[vk::location(1)]] float2 texcoord;
    [[vk::location(2)]] float4 color;
    [[vk::location(3)]] float4 viewPosition;
};


struct GeometryData {
    uint attributeCount;
    uint* indices;
    // Attribute buffers
    float3* position;
    float3* normal;
    float2* texcoord;
    float4* color;

    // Object to world transform
    float4x4 modelMatrix;
    float4x4 viewMatrix;
    float4x4 projectionMatrix;
    float4x4 modelNormalMatrix;
};

uint fetchIndex(GeometryData geom, uint attributeIndex, uint vertexIndex) {
    return geom.indices[geom.attributeCount * vertexIndex + attributeIndex];
}

T fetchAttribute<T>(GeometryData geom, T* data, uint attributeIndex, uint vertexIndex) {
    return data[fetchIndex(geom, attributeIndex, vertexIndex)];
}



[shader("vertex")]
VertexOutput geometryVertexShader(uint vertexID : SV_VertexID, uniform GeometryData geom) {
    let position = fetchAttribute(geom, geom.position, POSITION_ATTR, vertexID);
    let normal = fetchAttribute(geom, geom.normal, NORMAL_ATTR, vertexID);
    let texcoord = fetchAttribute(geom, geom.texcoord, TEXCOORD_ATTR, vertexID);
    let color = fetchAttribute(geom, geom.color, COLOR_ATTR, vertexID);

    let viewProjectionMatrix = mul(geom.projectionMatrix, geom.viewMatrix);
    let worldPosition = mul(geom.modelMatrix, float4(position, 1.0));
    let viewPosition = mul(geom.viewMatrix, worldPosition);
    let clipPosition = mul(geom.projectionMatrix, viewPosition);

    let worldNormal = mul(geom.modelNormalMatrix, float4(normal, 0.0));
    let viewNormal = normalize(mul(geom.viewMatrix, worldNormal).xyz);

    VertexOutput output;
    output.position = clipPosition;
    output.normal = viewNormal;
    output.texcoord = texcoord;
    output.viewPosition = viewPosition;


}