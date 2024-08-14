
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer intPtr { int d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uintPtr { uint d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer floatPtr { float d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec2Ptr { vec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec3Ptr { vec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec4Ptr { vec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec2Ptr { ivec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec3Ptr { ivec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec4Ptr { ivec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec2Ptr { uvec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec3Ptr { uvec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec4Ptr { uvec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat2Ptr { mat2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat3Ptr { mat3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat4Ptr { mat4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer intSlice { int[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uintSlice { uint[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer floatSlice { float[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec2Slice { vec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec3Slice { vec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec4Slice { vec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec2Slice { ivec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec3Slice { ivec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec4Slice { ivec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec2Slice { uvec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec3Slice { uvec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec4Slice { uvec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat2Slice { mat2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat3Slice { mat3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat4Slice { mat4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=4) coherent buffer image2DHandleSlice { image2DHandle[] d; };


layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer SceneParamsPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer SceneParamsSlice;

//  Scene camera parameters.
struct SceneParams {
    mat4 view;
    mat4 proj;
    mat4 viewProj;
    vec3 eye;
    float nearClip;
    float farClip;
    float left;
    float right;
    float top;
    float bottom;
    uvec2 viewportSize;
};

layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer SceneParamsPtr {SceneParams d;};
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer SceneParamsSlice {SceneParams[] d;};


layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ControlPointPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ControlPointSlice;

//  3D bezier control point.
struct ControlPoint {
    vec3 pos;
    vec3 color;
};

layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ControlPointPtr {ControlPoint d;};
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ControlPointSlice {ControlPoint[] d;};


layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer CurveDescPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer CurveDescSlice;

//  Represents a range of control points in the position buffer.
struct CurveDesc {
    vec4 widthProfile;
    vec4 opacityProfile;
    uint start;
    uint count;
    vec2 paramRange;
    uint brushIndex;
};

layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer CurveDescPtr {CurveDesc d;};
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer CurveDescSlice {CurveDesc[] d;};


layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokeVertexPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokeVertexSlice;

//  Stroke vertex.
struct StrokeVertex {
    vec3 pos;
    float s;
    u8vec4 color;
};

layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokeVertexPtr {StrokeVertex d;};
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokeVertexSlice {StrokeVertex[] d;};


layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokePtr;
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokeSlice;

//  Stroke vertex.
struct Stroke {
    uint baseVertex;
    uint vertexCount;
};

layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokePtr {Stroke d;};
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer StrokeSlice {Stroke[] d;};


//  Maximum number of line segments per tile.
const uint MAX_LINES_PER_TILE = 32;


struct TileLineData {
    vec4 lineCoords;
    vec2 paramRange;
    uint curveId;
    float depth;
};



layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer TileDataPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer TileDataSlice;

struct TileData {
    TileLineData[MAX_LINES_PER_TILE] lines;
};

layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer TileDataPtr {TileData d;};
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer TileDataSlice {TileData[] d;};


struct BinCurvesParams {
    ControlPointSlice controlPoints;
    CurveDescSlice curves;
    uintSlice tileLineCount;
    TileDataSlice tileData;
    SceneParamsPtr sceneParams;
    uvec2 viewportSize;
    float strokeWidth;
    uint baseCurveIndex;
    uint curveCount;
    uint tileCountX;
    uint tileCountY;
    uint frame;
};



struct TemporalAverageParams {
    uvec2 viewportSize;
    uint frame;
    float falloff;
    image2DHandle newFrame;
    image2DHandle avgFrame;
};



struct ComputeTestParams {
    uint elementCount;
    TileDataSlice data;
    ControlPointSlice controlPoints;
    image2DHandle outputImage;
};



struct DrawCurvesPushConstants {
    ControlPointSlice controlPoints;
    CurveDescSlice curves;
    SceneParamsPtr sceneParams;
    uint baseCurveIndex;
    float strokeWidth;
    uint tileCountX;
    uint tileCountY;
    uint frame;
    TileDataSlice tileData;
    uintSlice tileLineCount;
    image2DHandleSlice brushTextures;
    image2DHandle outputImage;
    uint debugOverflow;
    float strokeBleedExp;
};



const uint BINNING_TILE_SIZE = 16;


const uint DRAW_CURVES_WORKGROUP_SIZE_X = 16;


const uint DRAW_CURVES_WORKGROUP_SIZE_Y = 2;


const uint BINPACK_SUBGROUP_SIZE = 32;


const uint SUBGROUP_SIZE = 32;


const uint MAX_VERTICES_PER_CURVE = 64;


struct SummedAreaTableParams {
    uint pass;
    image2DHandle inputImage;
    image2DHandle outputImage;
};



struct Particle {
    u16vec3 pos;
};



struct ParticleCluster {
    vec3 pos;
    float size;
    uint count;
};



struct DrawStrokesPushConstants {
    StrokeVertexSlice vertices;
    StrokeSlice strokes;
    SceneParamsPtr sceneParams;
    image2DHandleSlice brushTextures;
    uint strokeCount;
    float width;
    float filterWidth;
};



