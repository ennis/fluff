// Bezier curve evaluation functions

struct CubicBezier2DSegment {
    vec2 p0;
    vec2 p1;
    vec2 p2;
    vec2 p3;
};

vec2 evalCubicBezier2D(CubicBezier2DSegment segment, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;
    return mt3 * segment.p0 + 3.0 * mt2 * t * segment.p1 + 3.0 * mt * t2 * segment.p2 + t3 * segment.p3;
}

vec2 evalCubicBezier2DTangent(CubicBezier2DSegment segment, float t) {
    float t2 = t * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    return 3.0 * mt2 * (segment.p1 - segment.p0) + 6.0 * mt * t * (segment.p2 - segment.p1) + 3.0 * t2 * (segment.p3 - segment.p2);
}

struct CubicBezier3DSegment {
    vec3 p0;
    vec3 p1;
    vec3 p2;
    vec3 p3;
};

vec3 evalCubicBezier3D(CubicBezier3DSegment segment, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;
    return mt3 * segment.p0 + 3.0 * mt2 * t * segment.p1 + 3.0 * mt * t2 * segment.p2 + t3 * segment.p3;
}

vec3 evalCubicBezier3DTangent(CubicBezier3DSegment segment, float t) {
    float t2 = t * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    return 3.0 * mt2 * (segment.p1 - segment.p0) + 6.0 * mt * t * (segment.p2 - segment.p1) + 3.0 * t2 * (segment.p3 - segment.p2);
}


struct RationalCubicBezier2DSegment {
    vec2 p0;
    vec2 p1;
    vec2 p2;
    vec2 p3;
    float w0;
    float w1;
    float w2;
    float w3;
};

RationalCubicBezier2DSegment projectCubicBezier3D(CubicBezier3DSegment segment, mat4 proj) {
    vec4 p0 = proj * vec4(segment.p0, 1.0);
    vec4 p1 = proj * vec4(segment.p1, 1.0);
    vec4 p2 = proj * vec4(segment.p2, 1.0);
    vec4 p3 = proj * vec4(segment.p3, 1.0);
    return RationalCubicBezier2DSegment(p0.xy / p0.w, p1.xy / p1.w, p2.xy / p2.w, p3.xy / p3.w, p0.w, p1.w, p2.w, p3.w);
}

vec2 evalRationalCubicBezier2D(RationalCubicBezier2DSegment segment, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;
    float w = mt3 * segment.w0 + 3.0 * mt2 * t * segment.w1 + 3.0 * mt * t2 * segment.w2 + t3 * segment.w3;
    return (mt3 * segment.w0 * segment.p0 + 3.0 * mt2 * t * segment.w1 * segment.p1 + 3.0 * mt * t2 * segment.w2 * segment.p2 + t3 * segment.w3 * segment.p3) / w;
}


struct RationalCubicBezier3DSegment {
    vec3 p0;
    vec3 p1;
    vec3 p2;
    vec3 p3;
    float w0;
    float w1;
    float w2;
    float w3;
};

/*
RationalCubicBezier3DSegment projectRationalCubicBezier3D(CubicBezier3DSegment segment, mat4 proj) {
    vec4 p0 = proj * vec4(segment.p0, 1.0);
    vec4 p1 = proj * vec4(segment.p1, 1.0);
    vec4 p2 = proj * vec4(segment.p2, 1.0);
    vec4 p3 = proj * vec4(segment.p3, 1.0);
    return RationalCubicBezier3DSegment(p0.xyz / p0.w, p1.xyz / p1.w, p2.xyz / p2.w, p3.xyz / p3.w, p0.w, p1.w, p2.w, p3.w);
}*/

vec3 evalRationalCubicBezier3D(RationalCubicBezier3DSegment segment, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;
    float w = mt3 * segment.w0 + 3.0 * mt2 * t * segment.w1 + 3.0 * mt * t2 * segment.w2 + t3 * segment.w3;
    return (mt3 * segment.w0 * segment.p0 + 3.0 * mt2 * t * segment.w1 * segment.p1 + 3.0 * mt * t2 * segment.w2 * segment.p2 + t3 * segment.w3 * segment.p3) / w;
}

vec3 evalRationalCubicBezier3DTangent(RationalCubicBezier3DSegment segment, float t) {
    float t2 = t * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    return 3.0 * mt2 * (segment.p1 - segment.p0) + 6.0 * mt * t * (segment.p2 - segment.p1) + 3.0 * t2 * (segment.p3 - segment.p2);
}
