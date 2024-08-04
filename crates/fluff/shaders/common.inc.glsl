// Various utility functions and constants

// IQ's palette function
vec3 palette(in float t, in vec3 a, in vec3 b, in vec3 c, in vec3 d)
{
    return a + b*cos(6.28318*(c*t+d));
}

// 2D Distance from a point to a segment.
float distSeg(vec2 p, vec2 a, vec2 b, out float alpha) {
    vec2 ab = b - a;
    vec2 ap = p - a;
    float side = sign(cross(vec3(ab, 0.0), vec3(ap, 0.0)).z);
    float d = dot(p - a, ab) / dot(ab, ab);
    d = clamp(d, 0.0, 1.0);
    vec2 p0 = a + d * ab;
    alpha = d;
    //float taper = max(0.0, 80.0 - distance(p,b)) / 80.0;
    return distance(p, p0);
}

// Noise stuff
float hash(in ivec2 p)
{
    // 2D -> 1D
    int n = p.x*3 + p.y*113;
    // 1D hash by Hugo Elias
    n = (n << 13) ^ n;
    n = n * (n * n * 15731 + 789221) + 1376312589;
    return -1.0+2.0*float(n & 0x0fffffff)/float(0x0fffffff);
}

float noise(vec2 p)
{
    ivec2 i = ivec2(floor(p));
    vec2 f = fract(p);
    // quintic interpolant
    vec2 u = f*f*f*(f*(f*6.0-15.0)+10.0);
    return mix(mix(hash(i + ivec2(0, 0)),
    hash(i + ivec2(1, 0)), u.x),
    mix(hash(i + ivec2(0, 1)),
    hash(i + ivec2(1, 1)), u.x), u.y);
}

float fbm(vec2 uv)
{
    float f = 0.0;
    uv /= 32.0;
    mat2 m = mat2(1.6, 1.2, -1.2, 1.6);
    f  = 0.5000*noise(uv); uv = m*uv;
    //f += 0.2500*noise( uv ); uv = m*uv;
    //f += 0.1250*noise( uv ); uv = m*uv;
    //f += 0.0625*noise( uv ); uv = m*uv;
    return f;
}

// Rounded-up integer division
uint divCeil(uint num, uint denom)
{
    return (num + denom - 1) / denom;
}

float remap(float value, float min1, float max1, float min2, float max2) {
    return min2 + (value - min1) * (max2 - min2) / (max1 - min1);
}

float linearstep(float edge0, float edge1, float x)
{
    return clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
}

// A range of control points in the controlPoints buffer that describes a single curve.
// 16+16+4+4+8 = 48 bytes
/*struct CurveDescriptor {
    vec4 widthProfile;// polynomial coefficients
    vec4 opacityProfile;// polynomial coefficients
    uint start;
    uint size;
    vec2 paramRange;
};

struct ControlPoint {
    vec3 pos;
    vec3 color;
};


layout(buffer_reference, buffer_reference_align=4) buffer CurveControlPoints {
    ControlPoint point;
};

layout(buffer_reference, buffer_reference_align=4) buffer CurveBuffer {
    CurveDescriptor curve;
};*/

/*
const int MAX_LINES_PER_TILE = 64;

struct TileEntry {
    vec4 line;
    vec2 paramRange;
    uint curveIndex;
};

/*struct Tile {
    TileEntry[MAX_LINES_PER_TILE] entries;
};*/

/*
layout(buffer_reference, scalar, buffer_reference_align=4) buffer TileLineCountData {
    int count;
};

layout(buffer_reference, scalar, buffer_reference_align=4) buffer TileData {
    TileEntry[MAX_LINES_PER_TILE] entries;
};

layout(buffer_reference, scalar, buffer_reference_align=8) buffer ControlPointBuffer {
    vec3 pos;
    vec3 color;
};

layout(buffer_reference, scalar, buffer_reference_align=8) buffer CurveBuffer {
    vec4 widthProfile;// polynomial coefficients
    vec4 opacityProfile;// polynomial coefficients
    uint start;
    uint size;
    vec2 paramRange;
};
*/