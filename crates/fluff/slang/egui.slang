/* #version 450 core

   layout(std140, push_constant) uniform PushConstants {
       mat4 matrix;
   };

   layout(location=0) in vec2 pos;
   layout(location=1) in vec2 uv;
   layout(location=2) in vec4 col;

   layout(location=0) out vec2 f_uv;
   layout(location=1) out vec4 f_color;

   // Built-in:
   // vec4 gl_Position

   void main() {
      gl_Position = vec4(
          2.0 * i_pos.x / u_screen_size.x - 1.0,
          2.0 * i_pos.y / u_screen_size.y - 1.0,
          0.0,
          1.0);
          o_color = linear_from_srgba(vec4(i_color));
          o_uv = i_uv;
   }

   */
struct PushConstants {
    float2 screenSize;
};

// 0-1 linear  from  0-255 sRGB
float3 linearFromSRGB(float3 srgb) {
    bool3 cutoff = srgb < float3(10.31475);
    float3 lower = srgb / float3(3294.6);
    float3 higher = pow((srgb + float3(14.025)) / float3(269.025), float3(2.4));
    return select(cutoff, lower, higher);
}

float4 linearFromSRGBA(float4 srgba) {
    return float4(linearFromSRGB(srgba.rgb), srgba.a / 255.0);
}

struct Vertex {
    [[vk::location(0)]] float2 pos;
    [[vk::location(1)]] float2 uv;
    [[vk::location(2)]] uint4 col;
};

struct VertexOutput {
    float4 pos: SV_Position;
    [[vk::location(0)]] float4 color;
    [[vk::location(1)]] float2 uv;
}

[shader("vertex")]
VertexOutput eguiVertexMain(Vertex v, uniform PushConstants u) {
    VertexOutput o;
    o.pos = float4(
        2.0 * v.pos.x / u.screenSize.x - 1.0,
        2.0 * v.pos.y / u.screenSize.y - 1.0,
        0.0,
        1.0);
    o.color = linearFromSRGBA(float4(v.col));
    o.uv = v.uv;
    return o;
}

/*#version 450 core

  layout(set=0,binding=0) uniform texture2D u_tex;
  layout(set=0,binding=1) uniform sampler u_sampler;

  layout(location=0) in vec4 i_color;
  layout(location=1) in vec2 i_uv;

  layout(location=0) out vec4 o_color;

  void main() {
      o_color = i_color * texture(sampler2D(u_tex,u_sampler), i_uv.st);
  }*/

struct FragmentOutput
{
    [[vk::location(0)]] float4 color;
};

[shader("fragment")]
FragmentOutput eguiFragMain(
    VertexOutput v,
    uniform Texture2D tex,
    uniform SamplerState sampler) {
    FragmentOutput o;
    o.color = v.color * tex.Sample(sampler, v.uv);   // texture(u_tex, v.uv);
    return o;
}