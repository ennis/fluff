// Greyscale 2D summed-area table calculation for brush masks, for one dimension at a time. (It's a prefix sum.)
#version 460 core
#include "bindless.inc.glsl"
#include "shared.inc.glsl"

const uint SAT_SIZE = 1 << SAT_LOG2_SIZE;

layout (local_size_x = SAT_SIZE, local_size_y = 1) in;

// One row or column of the SAT image.
shared float prefixSum[2 * SAT_SIZE];

layout(push_constant) uniform Constants {
    SummedAreaTableParams u;
};

void main() {

    ivec2 coord;
    if (u.pass == 0) {
        // Horizontal pass.
        coord = ivec2(gl_LocalInvocationID.x, gl_WorkGroupID.x);
    } else {
        // Vertical pass.
        coord = ivec2(gl_WorkGroupID.x, gl_LocalInvocationID.x);
    }

    prefixSum[gl_LocalInvocationID.x] = 1.0 - imageLoad(u.inputImage, coord).r;

    barrier();

    // The prefixSum array is in fact two arrays of size SAT_SIZE, and we ping-pong
    // between them in the steps. out_off is the offset in prefixSum where we write the
    // result of the current step.
    uint in_off = 0;
    uint out_off = SAT_SIZE;
    uint j = gl_LocalInvocationID.x;
    for (uint offset = 1; offset < SAT_SIZE; offset *= 2) {
        if (j >= offset) {
            prefixSum[out_off + j] = prefixSum[in_off + j] + prefixSum[in_off + j - offset];
        } else {
            prefixSum[out_off + j] = prefixSum[in_off + j];
        }
        in_off = SAT_SIZE - in_off;
        out_off = SAT_SIZE - out_off;
        barrier();
    }

    // Write the result.
    imageStore(u.outputImage, coord, prefixSum[out_off + j].xxxx);
}