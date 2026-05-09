struct Camera {
    view_proj:   mat4x4<f32>,
    sun_dir:     vec4<f32>, // xyz, w unused
    camera_pos:  vec4<f32>, // xyz, w unused
    fog_params:  vec4<f32>, // rgb color, density in w
};
@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexIn {
    @location(0) position:        vec3<f32>,
    @location(1) normal:          vec3<f32>,
    @location(2) instance_pos:    vec3<f32>,
    @location(3) instance_color:  vec3<f32>,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos:    vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) base_color:   vec3<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world_pos = in.position + in.instance_pos;
    out.clip_position = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_pos = world_pos;
    out.world_normal = in.normal;
    out.base_color = in.instance_color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let l = normalize(camera.sun_dir.xyz);
    let ambient = 0.45;
    let diffuse = max(dot(n, l), 0.0) * 0.85;
    let lit = in.base_color * (ambient + diffuse);

    let view_dist = length(in.world_pos - camera.camera_pos.xyz);
    let fog_density = camera.fog_params.w;
    let fog_factor = 1.0 - exp(-view_dist * fog_density);
    let final_rgb = mix(lit, camera.fog_params.rgb, clamp(fog_factor, 0.0, 1.0));

    return vec4<f32>(final_rgb, 1.0);
}
