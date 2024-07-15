//#extension GL_KHR_memory_scope_semantics : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_shader_explicit_arithmetic_types : require
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_EXT_shader_image_load_formatted : require
#extension GL_EXT_samplerless_texture_functions : require
#extension GL_EXT_buffer_reference : require
#extension GL_EXT_buffer_reference2 : require

layout(scalar) buffer;
layout(scalar) uniform;

layout(set=0, binding=0) uniform texture2D bindless_texture2D[];
layout(set=1, binding=0) uniform restrict image2D bindless_image2D[];
layout(set=1, binding=0) uniform restrict iimage2D bindless_iimage2D[];
layout(set=2, binding=0) uniform sampler bindless_sampler[];

// resource indexing structs
struct samplerIndex { uint idx; };
struct texture2DIndex { uint idx; };
struct image2DIndex { uint idx; };
struct iimage2DIndex { uint idx; };

/*
// Default sampler index
const samplerIndex s_linear_wrap = samplerIndex(0);
const samplerIndex s_linear_clamp = samplerIndex(1);
const samplerIndex s_nearest_wrap = samplerIndex(2);
const samplerIndex s_nearest_clamp = samplerIndex(3);*/

//-----------------------------------------------------------------------------
// sampler constructors
#define C_SAMPLER2D(tex, samp) sampler2D(bindless_texture2D[tex.idx], bindless_sampler[samp.idx])

//-----------------------------------------------------------------------------

vec4 sampleTexture2D(texture2DIndex tex, samplerIndex samp, vec2 P) { return texture(C_SAMPLER2D(tex, samp), P); }
vec4 imageLoad(image2DIndex image, ivec2 P) { return imageLoad(bindless_image2D[image.idx], P); }
void imageStore(image2DIndex image, ivec2 P, vec4 data) { imageStore(bindless_image2D[image.idx], P, data); }


#undef C_SAMPLER2D

/*
// function overrides
#if DIT_GL_ARB_sparse_texture2
int sparseTextureARB(sampler2D_u16 s, vec2 arg1, out vec4 arg2, float arg3){return sparseTextureARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
#endif
#if DIT_GL_ARB_sparse_texture_clamp
int sparseTextureClampARB(sampler2D_u16 s, vec2 arg1, float arg2, out vec4 arg3){return sparseTextureClampARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
int sparseTextureClampARB(sampler2D_u16 s, vec2 arg1, float arg2, out vec4 arg3, float arg4){return sparseTextureClampARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
#endif
#if DIT_GL_ARB_sparse_texture2
int sparseTextureGatherARB(sampler2D_u16 s, vec2 arg1, out vec4 arg2, int arg3, float arg4){return sparseTextureGatherARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
#endif
#if DIT_GL_ARB_sparse_texture_clamp
int sparseTextureGradClampARB(sampler2D_u16 s, vec2 arg1, vec2 arg2, vec2 arg3, float arg4, out vec4 arg5){return sparseTextureGradClampARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5); }
#endif
vec4 texture(sampler2D_u16 s, vec2 arg1, float arg2){return texture(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
#if DIT_GL_ARB_sparse_texture_clamp
vec4 textureClampARB(sampler2D_u16 s, vec2 arg1, float arg2){return textureClampARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 textureClampARB(sampler2D_u16 s, vec2 arg1, float arg2, float arg3){return textureClampARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
vec4 textureGradClampARB(sampler2D_u16 s, vec2 arg1, vec2 arg2, vec2 arg3, float arg4){return textureGradClampARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
#endif
vec4 textureProj(sampler2D_u16 s, vec3 arg1, float arg2){return textureProj(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 textureProj(sampler2D_u16 s, vec4 arg1, float arg2){return textureProj(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec2 textureQueryLod(sampler2D_u16 s, vec2 arg1){return textureQueryLod(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1); }
#if DIT_GL_ARB_sparse_texture2
int sparseTexelFetchARB(sampler2D_u16 s, ivec2 arg1, int arg2, out vec4 arg3){return sparseTexelFetchARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
int sparseTexelFetchARB(texture2D_u16 s, ivec2 arg1, int arg2, out vec4 arg3){return sparseTexelFetchARB(dit_texture2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
#endif
int sparseTexelGradFetchARB(sampler2D_u16 s, ivec2 arg1, int arg2, vec2 arg3, vec2 arg4, out vec4 arg5){return sparseTexelGradFetchARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5); }
#if DIT_GL_ARB_sparse_texture2
int sparseTextureARB(sampler2D_u16 s, vec2 arg1, out vec4 arg2){return sparseTextureARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
int sparseTextureGatherARB(sampler2D_u16 s, vec2 arg1, out vec4 arg2){return sparseTextureGatherARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
int sparseTextureGatherARB(sampler2D_u16 s, vec2 arg1, out vec4 arg2, int arg3){return sparseTextureGatherARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
#endif
#if DIT_GL_AMD_texture_gather_bias_lod
int sparseTextureGatherLodAMD(sampler2D_u16 s, vec2 arg1, float arg2, out vec4 arg3){return sparseTextureGatherLodAMD(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
int sparseTextureGatherLodAMD(sampler2D_u16 s, vec2 arg1, float arg2, out vec4 arg3, int arg4){return sparseTextureGatherLodAMD(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
#endif
#if DIT_GL_ARB_sparse_texture2
int sparseTextureGradARB(sampler2D_u16 s, vec2 arg1, vec2 arg2, vec2 arg3, out vec4 arg4){return sparseTextureGradARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
int sparseTextureLodARB(sampler2D_u16 s, vec2 arg1, float arg2, out vec4 arg3){return sparseTextureLodARB(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
#endif
vec4 texelFetch(sampler2D_u16 s, ivec2 arg1, int arg2){return texelFetch(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 texelFetch(texture2D_u16 s, ivec2 arg1, int arg2){return texelFetch(dit_texture2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
uvec4 texelFetch(usamplerBuffer_u16 s, int arg1){return texelFetch(dit_usamplerBuffer[nonuniformEXT(uint(s.idx))], arg1); }
vec4 texelGradFetch(sampler2D_u16 s, ivec2 arg1, int arg2, vec2 arg3, vec2 arg4){return texelGradFetch(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
vec4 texelProjFetch(sampler2D_u16 s, vec4 arg1, int arg2){return texelProjFetch(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 texelProjFetch(sampler2D_u16 s, ivec3 arg1, int arg2){return texelProjFetch(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 texelProjGradFetch(sampler2D_u16 s, vec4 arg1, int arg2, vec2 arg3, vec2 arg4){return texelProjGradFetch(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
vec4 texelProjGradFetch(sampler2D_u16 s, ivec3 arg1, int arg2, vec2 arg3, vec2 arg4){return texelProjGradFetch(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
vec4 texture(sampler2D_u16 s, vec2 arg1){return texture(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1); }
#if DIT_GL_NV_shader_texture_footprint
bool textureFootprintClampNV(sampler2D_u16 s, vec2 arg1, float arg2, int arg3, bool arg4, out gl_TextureFootprint2DNV arg5){return textureFootprintClampNV(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5); }
bool textureFootprintClampNV(sampler2D_u16 s, vec2 arg1, float arg2, int arg3, bool arg4, out gl_TextureFootprint2DNV arg5, float arg6){return textureFootprintClampNV(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5, arg6); }
bool textureFootprintGradClampNV(sampler2D_u16 s, vec2 arg1, vec2 arg2, vec2 arg3, float arg4, int arg5, bool arg6, out gl_TextureFootprint2DNV arg7){return textureFootprintGradClampNV(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5, arg6, arg7); }
bool textureFootprintGradNV(sampler2D_u16 s, vec2 arg1, vec2 arg2, vec2 arg3, int arg4, bool arg5, out gl_TextureFootprint2DNV arg6){return textureFootprintGradNV(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5, arg6); }
bool textureFootprintLodNV(sampler2D_u16 s, vec2 arg1, float arg2, int arg3, bool arg4, out gl_TextureFootprint2DNV arg5){return textureFootprintLodNV(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5); }
bool textureFootprintNV(sampler2D_u16 s, vec2 arg1, int arg2, bool arg3, out gl_TextureFootprint2DNV arg4){return textureFootprintNV(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4); }
bool textureFootprintNV(sampler2D_u16 s, vec2 arg1, int arg2, bool arg3, out gl_TextureFootprint2DNV arg4, float arg5){return textureFootprintNV(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3, arg4, arg5); }
#endif
vec4 textureGather(sampler2D_u16 s, vec2 arg1){return textureGather(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1); }
#if DIT_GL_AMD_texture_gather_bias_lod
vec4 textureGatherLodAMD(sampler2D_u16 s, vec2 arg1, float arg2){return textureGatherLodAMD(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 textureGatherLodAMD(sampler2D_u16 s, vec2 arg1, float arg2, int arg3){return textureGatherLodAMD(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
#endif
vec4 textureGrad(sampler2D_u16 s, vec2 arg1, vec2 arg2, vec2 arg3){return textureGrad(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
vec4 textureLod(sampler2D_u16 s, vec2 arg1, float arg2){return textureLod(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 textureProj(sampler2D_u16 s, vec3 arg1){return textureProj(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1); }
vec4 textureProj(sampler2D_u16 s, vec4 arg1){return textureProj(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1); }
vec4 textureProjGrad(sampler2D_u16 s, vec3 arg1, vec2 arg2, vec2 arg3){return textureProjGrad(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
vec4 textureProjGrad(sampler2D_u16 s, vec4 arg1, vec2 arg2, vec2 arg3){return textureProjGrad(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2, arg3); }
vec4 textureProjLod(sampler2D_u16 s, vec3 arg1, float arg2){return textureProjLod(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
vec4 textureProjLod(sampler2D_u16 s, vec4 arg1, float arg2){return textureProjLod(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1, arg2); }
int textureQueryLevels(sampler2D_u16 s){return textureQueryLevels(dit_sampler2D[nonuniformEXT(uint(s.idx))]); }
int textureQueryLevels(texture2D_u16 s){return textureQueryLevels(dit_texture2D[nonuniformEXT(uint(s.idx))]); }
ivec2 textureSize(sampler2D_u16 s, int arg1){return textureSize(dit_sampler2D[nonuniformEXT(uint(s.idx))], arg1); }
ivec2 textureSize(texture2D_u16 s, int arg1){return textureSize(dit_texture2D[nonuniformEXT(uint(s.idx))], arg1); }
int textureSize(usamplerBuffer_u16 s){return textureSize(dit_usamplerBuffer[nonuniformEXT(uint(s.idx))]); }
*/