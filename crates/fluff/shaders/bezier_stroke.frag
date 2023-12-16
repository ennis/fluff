#version 450
#extension GL_NV_uniform_buffer_std430_layout : enable

#pragma include <common.glsl>

struct BezPoint {
    vec2 pos;
    float t;
    float pad;
};

layout(std430 ,binding=0) uniform Uniforms {
    vec2[MAX_CONTROL_POINTS] controlPoints;
    BezPoint[BEZIER_PROJECTION_LUT_SIZE] pointLut;
    int controlPointCount;
    int lutPointCount;
    float screenWidth;
    float screenHeight;
};

layout(location=0) in vec2 position;
layout(location=0) out vec4 out_color;



void bezierProj(Bezier bez, vec2 pos, out vec2 closestPoint, out float t, out float closestDist) {

    // initial search
    const int maxStep = 16;      // max step for the initial search
    float minDist = 99999999.; // current minimum distance
    int index = 0; // index of the closest point
    for (int i = 0; i <= maxStep; ++i) {
        float d = distance(pos, bezier3(bez, float(i) / maxStep));
        if (d < minDist) {
            index = i;
            minDist = d;
        }
    }

    float t_prev = float(max(index - 1, 0))  / maxStep ;
    float t_next = float(min(index + 1, maxStep) ) / maxStep;
    float t_mid = float(index) / maxStep;

    BezPoint points[3] = BezPoint[3](
            BezPoint(bezier3(bez, t_prev), t_prev, 0.0),
            BezPoint(bezier3(bez, t_mid), t_mid, 0.0),
            BezPoint(bezier3(bez, t_next), t_next, 0.0));

    // refinement
    const int numRefineSteps = 6;
    for (int i = 0; i < numRefineSteps; ++i) {
        BezPoint prev = points[0];
        BezPoint next = points[2];
        BezPoint mid = points[1];

        vec2 mid_prev = bezier3(bez, .5 * (prev.t + mid.t)); // halfway bewteen prev and mid
        vec2 mid_next = bezier3(bez, .5 * (next.t + mid.t)); // halfway between next and mid

        float d_prev = distance(pos, mid_prev);
        float d_next = distance(pos, mid_next);
        float d_mid = distance(pos, mid.pos);

        // find the closest point between the three
        float md = 99999999.;
        int closest = 0;
        if (d_prev < md) {
            md = d_prev;
            closest = 0;
        }
        if (d_mid < md) {
            md = d_mid;
            closest = 1;
        }
        if (d_next < md) {
            closest = 2;
        }

        if (closest == 0) {
            points[2] = mid;
            points[1] = BezPoint(mid_prev, .5 * (prev.t + mid.t), 0.);
        } else if (closest == 1) {
            points[0] = BezPoint(mid_prev, .5 * (prev.t + mid.t), 0.);
            points[2] = BezPoint(mid_next, .5 * (next.t + mid.t), 0.);
        } else {
            points[0] = mid;
            points[1] = BezPoint(mid_next, .5 * (next.t + mid.t), 0.);
        }
    }

    closestPoint = points[1].pos;
    t = points[1].t;
    closestDist = distance(pos, closestPoint);
}

void main() {
    vec2 uv = 0.5 * vec2(position.x, position.y) + 0.5;
    vec2 resolution = vec2(screenWidth, screenHeight);
    vec2 screenPos = uv * resolution;
    screenPos.y = resolution.y - screenPos.y;


    Bezier bez = Bezier(controlPoints[0], controlPoints[1], controlPoints[2], controlPoints[3]);

    vec2 closestPoint;
    float t;
    float closestDist;
    bezierProj(bez, screenPos, closestPoint, t, closestDist);

    vec2 tangent = bezier3Tangent(bez, t);
    float profile = 80. * exp(-5. * t);
    vec3 color = palette(t,
                          vec3(0.5, 0.5, 0.5),
                          vec3(0.5, 0.5, 0.5),
                          vec3(1.0, 0.7, 0.4),
                          vec3(0.00, 0.15, 0.20));

    float dist = closestDist; //* sign(cross(vec3(tangent, 0.), vec3(screenPos - closestPoint, 0.)).z);
    float aastep = 0.7*fwidth(dist);
   // dist += aastep * 5.0 * (fbm(screenPos));
    float mask = 1.0-smoothstep(profile-aastep, profile+aastep, dist);

    //out_color = vec4(index == 0, index == 1, index == 2, 1.0);
    out_color = vec4(max(dist,0.0)/1000, -min(0.0,dist)/1000, 0.0, 1.0);
    out_color = vec4(mask * color, 1.0);
}
