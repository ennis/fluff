import "shared.slang";

// ------ OUTPUT ------

[shader("compute")]
[numthreads(16, 16)]
void temporalAverage(
    uint3 threadID : SV_DispatchThreadID,
    uniform TemporalAverageParams u)
{
    int2 coord = threadID.xy;
    if (coord.x >= u.viewportSize.x || coord.y >= u.viewportSize.y) {
        return;
    }

    float4 newColor = u.newFrame.Load(coord);        // imageLoad(new_frame, ivec2(coord));
    float4 avgColor = u.avgFrame.Load(coord);    // imageLoad(avg_frame, ivec2(coord));
    // exponential moving average
    float4 newAvg = lerp(avgColor, newColor, u.falloff);
    u.avgFrame.Store(coord, newAvg);    //  imageStore(avg_frame, coord, newAvg);
}