------------------------------------------------------------------------
-- Prelude
--

G_images = {}
G_buffers = {}
G_passes = {}

function render_target(props)
    G_images[props.id] = props
end

function buffer(props)
    G_buffers[props.id] = props
end

function passes(passes)
    G_passes = passes
end

--
function fill_buffer(props)
    props.type = 'fill_buffer'
    return props
end

--
function mesh_shading(props)
    props.type = 'mesh_shading'
    return props
end

--
function compute(props)
    props.type = 'compute'
    return props
end

function global_definitions(defs)
    for k, v in pairs(defs) do
        _G[k] = v
    end
end

------------------------------------------------------------------------
-- Render configuration
--

global_definitions {
    TILE_WIDTH = 16,
    TILE_HEIGHT = 16,
    CURVE_BINNING_TASK_WORKGROUP_SIZE = 64,
}

render_target {
    id = 'RT_depth',
    format = 'depth32float',
}

render_target {
    id = 'RT_color',
    format = 'rgba8unorm',
}

render_target {
    id = 't_tileLineCount',
    format = 'r32sint',
    width_div = TILE_WIDTH,
    height_div = TILE_HEIGHT,
}

buffer {
    id = 'b_tileData',
    width_div = TILE_WIDTH,
    height_div = TILE_HEIGHT,
}

-- Curve control points
buffer { id = 'b_controlPoints' }

-- Curve descriptors (start, end in control points array)
buffer { id = 'b_curveData' }

passes {
    -- clear the tile buffer
    fill_buffer {
        id = 'CLEAR_TILE_BUFFER',
        buffer = 'BUF_tileBuffer',
        value = {
            uint = 0,
        },
    },

    -- coarse shading pass
    mesh_shading {
        id = 'CURVE_BINNING',
        shader = { file = 'crates/fluff/shaders/bin_curves.glsl' },
        rasterizerState = {
            polygonMode = 'fill',
            cullMode = 'back',
            frontFace = 'ccw',
        },
        depthStencilState = {
            depthTest = true,
            depthWriteEnable = true,
            depthCompare = 'always'
        },
        colorAttachments = {
            [0] = {
                target = 'RT_tileLineCount',
                blendEnable = false,
                clear = {
                    uint = 0
                },
            },
        },

        draw = function(renderData)
            return {
                groupCountX = math.ceil(renderData.curveCount / CURVE_BINNING_TASK_WORKGROUP_SIZE),
                groupCountY = 1,
                groupCountZ = 1,
            }
        end,
    },

    -- curve rendering pass
    compute {
        shader = { file = 'crates/fluff/shaders/curve_binning_render.comp' }
    }
}