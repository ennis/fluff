
layout(buffer_reference, scalar, buffer_reference_align=8) buffer intPtr { int d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uintPtr { uint d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer floatPtr { float d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer vec2Ptr { vec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer vec3Ptr { vec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer vec4Ptr { vec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ivec2Ptr { ivec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ivec3Ptr { ivec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ivec4Ptr { ivec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uvec2Ptr { uvec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uvec3Ptr { uvec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uvec4Ptr { uvec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer mat2Ptr { mat2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer mat3Ptr { mat3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer mat4Ptr { mat4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer intSlice { int[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uintSlice { uint[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer floatSlice { float[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer vec2Slice { vec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer vec3Slice { vec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer vec4Slice { vec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ivec2Slice { ivec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ivec3Slice { ivec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ivec4Slice { ivec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uvec2Slice { uvec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uvec3Slice { uvec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer uvec4Slice { uvec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer mat2Slice { mat2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer mat3Slice { mat3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) buffer mat4Slice { mat4[] d; };


layout(buffer_reference, scalar, buffer_reference_align=8) buffer SceneParamsPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) buffer SceneParamsSlice;

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
};

layout(buffer_reference, scalar, buffer_reference_align=8) buffer SceneParamsPtr {SceneParams d;};
layout(buffer_reference, scalar, buffer_reference_align=8) buffer SceneParamsSlice {SceneParams[] d;};


layout(buffer_reference, scalar, buffer_reference_align=8) buffer ControlPointPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ControlPointSlice;

//  3D bezier control point.
struct ControlPoint {
    vec3 pos;
    vec3 color;
};

layout(buffer_reference, scalar, buffer_reference_align=8) buffer ControlPointPtr {ControlPoint d;};
layout(buffer_reference, scalar, buffer_reference_align=8) buffer ControlPointSlice {ControlPoint[] d;};


layout(buffer_reference, scalar, buffer_reference_align=8) buffer CurveDescPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) buffer CurveDescSlice;

//  Represents a range of control points in the position buffer.
struct CurveDesc {
    vec4 widthProfile;
    vec4 opacityProfile;
    uint start;
    uint count;
    vec2 paramRange;
};

layout(buffer_reference, scalar, buffer_reference_align=8) buffer CurveDescPtr {CurveDesc d;};
layout(buffer_reference, scalar, buffer_reference_align=8) buffer CurveDescSlice {CurveDesc[] d;};


//  Maximum number of line segments per tile.
const uint MAX_LINES_PER_TILE = 32;


struct TileLineData {
    vec4 lineCoords;
    vec2 paramRange;
    uint curveId;
    float depth;
};



layout(buffer_reference, scalar, buffer_reference_align=8) buffer TileDataPtr;
layout(buffer_reference, scalar, buffer_reference_align=8) buffer TileDataSlice;

struct TileData {
    TileLineData[MAX_LINES_PER_TILE] lines;
};

layout(buffer_reference, scalar, buffer_reference_align=8) buffer TileDataPtr {TileData d;};
layout(buffer_reference, scalar, buffer_reference_align=8) buffer TileDataSlice {TileData[] d;};


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
    texture2DRange brushTextures;
    image2DHandle outputImage;
    uint debugOverflow;
    float strokeBleedExp;
};



const uint BINNING_TILE_SIZE = 16;


const uint DRAW_CURVES_WORKGROUP_SIZE_X = 16;


const uint DRAW_CURVES_WORKGROUP_SIZE_Y = 2;


const uint BINPACK_SUBGROUP_SIZE = 32;


const uint MAX_VERTICES_PER_CURVE = 64;


const uint SAT_LOG2_SIZE = 7;


struct SummedAreaTableParams {
    uint pass;
    image2DHandle inputImage;
    image2DHandle outputImage;
};



