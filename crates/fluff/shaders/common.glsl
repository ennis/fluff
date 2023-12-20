
struct Bezier {
    vec2 p0;
    vec2 p1;
    vec2 p2;
    vec2 p3;
};

vec2 bezier3(Bezier bez, float t) {
    float t2 = t * t;
    float t3 = t2 * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;
    float mt3 = mt2 * mt;

    return mt3 * bez.p0 + 3.0 * mt2 * t * bez.p1 + 3.0 * mt * t2 * bez.p2 + t3 * bez.p3;
}

vec2 bezier3Tangent(Bezier bez, float t) {
    float t2 = t * t;
    float mt = 1.0 - t;
    float mt2 = mt * mt;

    return 3.0 * mt2 * (bez.p1 - bez.p0) + 6.0 * mt * t * (bez.p2 - bez.p1) + 3.0 * t2 * (bez.p3 - bez.p2);
}


////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
vec3 palette( in float t, in vec3 a, in vec3 b, in vec3 c, in vec3 d )
{
    return a + b*cos( 6.28318*(c*t+d) );
}

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

// Noise stuff
float hash(in ivec2 p)  // this hash is not production ready, please
{                         // replace this by something better

    // 2D -> 1D
    int n = p.x*3 + p.y*113;

    // 1D hash by Hugo Elias
    n = (n << 13) ^ n;
    n = n * (n * n * 15731 + 789221) + 1376312589;
    return -1.0+2.0*float( n & 0x0fffffff)/float(0x0fffffff);
}

float noise(vec2 p)
{
    ivec2 i = ivec2(floor( p ));
    vec2 f = fract( p );

    // quintic interpolant
    vec2 u = f*f*f*(f*(f*6.0-15.0)+10.0);

    return mix( mix( hash( i + ivec2(0,0) ),
                     hash( i + ivec2(1,0) ), u.x),
                mix( hash( i + ivec2(0,1) ),
                     hash( i + ivec2(1,1) ), u.x), u.y);
}

float fbm(vec2 uv)
{
    float f = 0.0;
    uv /= 32.0;
    mat2 m = mat2( 1.6,  1.2, -1.2,  1.6 );
    f  = 0.5000*noise( uv ); uv = m*uv;
    //f += 0.2500*noise( uv ); uv = m*uv;
    //f += 0.1250*noise( uv ); uv = m*uv;
    //f += 0.0625*noise( uv ); uv = m*uv;
    return f;
}
