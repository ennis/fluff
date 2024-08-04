// Bezier curve evaluation functions

struct CubicBezier2D {
    vec2 p0;
    vec2 p1;
    vec2 p2;
    vec2 p3;
};

vec2 evalCubicBezier2D(CubicBezier2D c, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;
    return mt3 * c.p0 + 3.0 * mt2 * t * c.p1 + 3.0 * mt * t2 * c.p2 + t3 * c.p3;
}

vec2 evalCubicBezier2D_T(CubicBezier2D c, float t) {
    float t2 = t * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    return 3.0 * mt2 * (c.p1 - c.p0) + 6.0 * mt * t * (c.p2 - c.p1) + 3.0 * t2 * (c.p3 - c.p2);
}

struct CubicBezier3D {
    vec3 p0;
    vec3 p1;
    vec3 p2;
    vec3 p3;
};

vec3 evalCubicBezier3D(CubicBezier3D segment, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;
    return mt3 * segment.p0 + 3.0 * mt2 * t * segment.p1 + 3.0 * mt * t2 * segment.p2 + t3 * segment.p3;
}

vec3 evalCubicBezier3DTangent(CubicBezier3D segment, float t) {
    float t2 = t * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    return 3.0 * mt2 * (segment.p1 - segment.p0) + 6.0 * mt * t * (segment.p2 - segment.p1) + 3.0 * t2 * (segment.p3 - segment.p2);
}

// Rational 3D cubic bezier curve
struct RCubicBezier3D {
    vec4 p0;
    vec4 p1;
    vec4 p2;
    vec4 p3;
};

RCubicBezier3D projectCubicBezier3D(CubicBezier3D c, mat4 proj) {
    return RCubicBezier3D(
    proj * vec4(c.p0, 1.0),
    proj * vec4(c.p1, 1.0),
    proj * vec4(c.p2, 1.0),
    proj * vec4(c.p3, 1.0));
}

vec4 evalRCubicBezier3D(RCubicBezier3D c, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;
    vec4 v = mt3 * c.p0 + 3.0 * mt2 * t * c.p1 + 3.0 * mt * t2 * c.p2 + t3 * c.p3;
    return v.xyzw;
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
