// Given we render 3 verticies numbered (0, 1, 2)
// this function creates a triangle with the coords
// -1.0 + ((0 & 1) << 2), -1.0 + ((0 & 2) << 1) => (-1.0, -1.0)
// -1.0 + ((1 & 1) << 2), -1.0 + ((1 & 2) << 1) => ( 3.0, -1.0)
// -1.0 + ((2 & 1) << 2), -1.0 + ((2 & 2) << 1) => (-1.0,  3.0)
@vertex
fn vert_main(@builtin(vertex_index) in_vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = -1.0 + f32((in_vertex_index & u32(1)) << u32(2));
    let y = -1.0 + f32((in_vertex_index & u32(2)) << u32(1));
    return vec4<f32>(x, y, 0.0, 1.0);
}

// [[group(0), binding(0)]] 
// var in_texture: [[access(read)]] texture_storage_2d<rgba32float>;

@fragment
fn frag_main(@builtin(position) coord_in: vec4<f32>) -> @location(0) vec4<f32> {
    // let pixel_color = textureLoad(in_texture, vec2<i32>(coord_in.xy));
    // return pixel_color;
    return vec4<f32>(coord_in.x, coord_in.y, 0.1, 1.0);
}
