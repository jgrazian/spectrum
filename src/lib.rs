use std::sync::Arc;

use anyhow::Result;
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    window: Arc<Window>,
    surface_configured: bool,
}

impl State {
    async fn new(window: Arc<Window>) -> State {
        let size = window.inner_size();

        // The instance is a handle to our GPU
        // BackendBit::PRIMARY => Vulkan + Metal + DX12 + Browser WebGPU
        let instance_desc = wgpu::InstanceDescriptor {
            #[cfg(target_arch = "wasm32")]
            backends: if cfg!(not(target_arch = "wasm32")) {
                wgpu::Backends::PRIMARY
            } else {
                wgpu::Backends::GL
            },
            ..Default::default()
        };
        let instance = wgpu::Instance::new(instance_desc);

        let surface = instance.create_surface(window.clone()).unwrap();

        for adapter in instance.enumerate_adapters(wgpu::Backends::all()) {
            println!("{:?}", adapter.get_info())
        }

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let device_desc = wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            // WebGL doesn't support all of wgpu's features, so if
            // we're building for the web we'll have to disable some.
            required_limits: if cfg!(target_arch = "wasm32") {
                wgpu::Limits::downlevel_webgl2_defaults()
            } else {
                wgpu::Limits::default()
            },
            memory_hints: wgpu::MemoryHints::default(),
        };
        let (device, queue) = adapter.request_device(&device_desc, None).await.unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an Srgb surface texture. Using a different
        // one will result all the colors comming out darker. If you want to support non
        // Srgb surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            desired_maximum_frame_latency: 2,
            view_formats: vec![],
        };

        let surface_configured;
        #[cfg(not(target_arch = "wasm32"))]
        {
            surface.configure(&device, &config);
            surface_configured = true;
        }
        #[cfg(target_arch = "wasm32")]
        {
            surface_configured = false;
        }

        Self {
            surface,
            device,
            queue,
            config,
            size,
            window,
            surface_configured,
        }
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    fn input(&mut self, _: &WindowEvent) -> bool {
        false
    }

    fn update(&mut self) {}

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let clear_color = wgpu::Color {
                r: 0.1,
                g: 0.2,
                b: 0.3,
                a: 1.0,
            };
            let color_attachment = wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            };
            let render_pass_desc = wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(color_attachment)],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            };
            let _render_pass = encoder.begin_render_pass(&render_pass_desc);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

enum UserEvent {
    StateReady(State),
}

struct App {
    state: Option<State>,
    event_loop_proxy: EventLoopProxy<UserEvent>,
}

impl App {
    fn new(event_loop: &EventLoop<UserEvent>) -> Self {
        Self {
            state: None,
            event_loop_proxy: event_loop.create_proxy(),
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        tracing::info!("Resumed");
        let window_attrs = Window::default_attributes();
        let window = event_loop
            .create_window(window_attrs)
            .expect("Couldn't create window.");

        #[cfg(target_arch = "wasm32")]
        {
            use web_sys::Element;
            use winit::{dpi::PhysicalSize, platform::web::WindowExtWebSys};

            web_sys::window()
                .and_then(|win| win.document())
                .and_then(|doc| {
                    let dst = doc.get_element_by_id("wasm-example")?;
                    let canvas = Element::from(window.canvas()?);
                    dst.append_child(&canvas).ok()?;
                    Some(())
                })
                .expect("Couldn't append canvas to document body.");

            // Winit prevents sizing with CSS, so we have to set
            // the size manually when on web.
            let _ = window.request_inner_size(PhysicalSize::new(450, 400));

            let state_future = State::new(Arc::new(window));
            let event_loop_proxy = self.event_loop_proxy.clone();
            let future = async move {
                let state = state_future.await;
                assert!(event_loop_proxy
                    .send_event(UserEvent::StateReady(state))
                    .is_ok());
            };
            wasm_bindgen_futures::spawn_local(future)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let state = pollster::block_on(State::new(Arc::new(window)));
            assert!(self
                .event_loop_proxy
                .send_event(UserEvent::StateReady(state))
                .is_ok());
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::StateReady(state) = event;
        self.state = Some(state);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(ref mut state) = self.state else {
            return;
        };

        if window_id != state.window.id() {
            return;
        }

        if state.input(&event) {
            return;
        }

        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    },
                ..
            } => {
                tracing::info!("Exited!");
                event_loop.exit()
            }
            WindowEvent::Resized(physical_size) => {
                tracing::info!("physical_size: {physical_size:?}");
                state.surface_configured = true;
                state.resize(physical_size);
            }
            WindowEvent::RedrawRequested => {
                if !state.surface_configured {
                    return;
                }
                state.update();
                match state.render() {
                    Ok(()) => {}
                    // Reconfigure the surface if it's lost or outdated
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        state.resize(state.size);
                    }
                    // The system is out of memory, we should probably quit
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        tracing::error!("OutOfMemory");
                        event_loop.exit();
                    }

                    // This happens when the frame takes too long to present
                    Err(wgpu::SurfaceError::Timeout) => {
                        tracing::warn!("Surface timeout");
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(ref state) = self.state {
            state.window.request_redraw();
        };
    }
}

pub fn run() -> Result<()> {
    let env_filter = EnvFilter::builder()
        .with_default_directive(Level::INFO.into())
        .from_env_lossy()
        .add_directive("wgpu_core::device::resource=warn".parse()?);
    let subscriber = tracing_subscriber::registry().with(env_filter);
    #[cfg(target_arch = "wasm32")]
    {
        use tracing_wasm::{WASMLayer, WASMLayerConfig};

        console_error_panic_hook::set_once();
        let wasm_layer = WASMLayer::new(WASMLayerConfig::default());

        subscriber.with(wasm_layer).init();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let fmt_layer = tracing_subscriber::fmt::Layer::default();
        subscriber.with(fmt_layer).init();
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut app = App::new(&event_loop);

    event_loop.run_app(&mut app)?;
    Ok(())
}

// struct State<'a> {
//     surface: wgpu::Surface<'a>,
//     device: wgpu::Device,
//     queue: wgpu::Queue,
//     config: wgpu::SurfaceConfiguration,
//     size: winit::dpi::PhysicalSize<u32>,
//     window: &'a Window,
//     render_pipeline: wgpu::RenderPipeline,
//     output_textures: [wgpu::Texture; 2],
//     bind_groups: [wgpu::BindGroup; 2],
// }

// impl<'a> State<'a> {
//     // Creating some of the wgpu types requires async code
//     async fn new(window: &'a Window) -> State<'a> {
//         let size = window.inner_size();

//         // The instance is a handle to our GPU
//         // Backends::all => Vulkan + Metal + DX12 + Browser WebGPU
//         let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
//             #[cfg(not(target_arch = "wasm32"))]
//             backends: wgpu::Backends::PRIMARY,
//             #[cfg(target_arch = "wasm32")]
//             backends: wgpu::Backends::GL,
//             ..Default::default()
//         });

//         let surface = instance.create_surface(window).unwrap();

//         let adapter = instance
//             .request_adapter(&wgpu::RequestAdapterOptions {
//                 power_preference: wgpu::PowerPreference::default(),
//                 compatible_surface: Some(&surface),
//                 force_fallback_adapter: false,
//             })
//             .await
//             .unwrap();

//         let (device, queue) = adapter
//             .request_device(
//                 &wgpu::DeviceDescriptor {
//                     required_features: wgpu::Features::empty(),
//                     // WebGL doesn't support all of wgpu's features, so if
//                     // we're building for the web, we'll have to disable some.
//                     required_limits: if cfg!(target_arch = "wasm32") {
//                         wgpu::Limits::downlevel_webgl2_defaults()
//                     } else {
//                         wgpu::Limits::default()
//                     },
//                     label: None,
//                     memory_hints: wgpu::MemoryHints::Performance,
//                 },
//                 None, // Trace path
//             )
//             .await
//             .unwrap();

//         let surface_caps = surface.get_capabilities(&adapter);

//         let surface_format = surface_caps
//             .formats
//             .iter()
//             .find(|f| f.is_srgb())
//             .copied()
//             .unwrap_or(surface_caps.formats[0]);

//         let config = wgpu::SurfaceConfiguration {
//             usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
//             format: surface_format,
//             width: size.width,
//             height: size.height,
//             present_mode: surface_caps.present_modes[0],
//             alpha_mode: surface_caps.alpha_modes[0],
//             view_formats: vec![],
//             desired_maximum_frame_latency: 2,
//         };

//         let bind_group_layout_desc = wgpu::BindGroupLayoutDescriptor {
//             label: Some("bind group layout"),
//             entries: &[
//                 wgpu::BindGroupLayoutEntry {
//                     binding: 0,
//                     visibility: ShaderStages::COMPUTE | ShaderStages::FRAGMENT,
//                     ty: wgpu::BindingType::Texture {
//                         sample_type: wgpu::TextureSampleType::Float { filterable: false },
//                         view_dimension: wgpu::TextureViewDimension::D2,
//                         multisampled: false,
//                     },
//                     count: None,
//                 },
//                 wgpu::BindGroupLayoutEntry {
//                     binding: 1,
//                     visibility: ShaderStages::COMPUTE | ShaderStages::FRAGMENT,
//                     ty: wgpu::BindingType::Texture {
//                         sample_type: wgpu::TextureSampleType::Float { filterable: false },
//                         view_dimension: wgpu::TextureViewDimension::D2,
//                         multisampled: false,
//                     },
//                     count: None,
//                 },
//             ],
//         };
//         let bind_group_layout = device.create_bind_group_layout(&bind_group_layout_desc);

//         // 1byte * 4 channels * width * height
//         let image_size = size.width as usize * size.height as usize * 4 * 4;
//         let output_textures = [
//             device.create_texture_with_data(
//                 &queue,
//                 &wgpu::TextureDescriptor {
//                     label: Some("image texture 1"),
//                     size: wgpu::Extent3d {
//                         width: size.width,
//                         height: size.height,
//                         depth_or_array_layers: 1,
//                     },
//                     mip_level_count: 1,
//                     sample_count: 1,
//                     dimension: wgpu::TextureDimension::D2,
//                     format: wgpu::TextureFormat::Bgra8UnormSrgb,
//                     usage: wgpu::TextureUsages::TEXTURE_BINDING,
//                     view_formats: &[wgpu::TextureFormat::Bgra8UnormSrgb],
//                 },
//                 wgpu::util::TextureDataOrder::LayerMajor,
//                 &vec![255; image_size],
//             ),
//             device.create_texture_with_data(
//                 &queue,
//                 &wgpu::TextureDescriptor {
//                     label: Some("image texture 2"),
//                     size: wgpu::Extent3d {
//                         width: size.width,
//                         height: size.height,
//                         depth_or_array_layers: 1,
//                     },
//                     mip_level_count: 1,
//                     sample_count: 1,
//                     dimension: wgpu::TextureDimension::D2,
//                     format: wgpu::TextureFormat::Bgra8UnormSrgb,
//                     usage: wgpu::TextureUsages::TEXTURE_BINDING,
//                     view_formats: &[wgpu::TextureFormat::Bgra8UnormSrgb],
//                 },
//                 wgpu::util::TextureDataOrder::LayerMajor,
//                 &vec![128; image_size],
//             ),
//         ];
//         let texture_view_1 = output_textures[0].create_view(&wgpu::TextureViewDescriptor {
//             format: Some(wgpu::TextureFormat::Bgra8UnormSrgb),
//             ..wgpu::TextureViewDescriptor::default()
//         });
//         let texture_view_2 = output_textures[1].create_view(&wgpu::TextureViewDescriptor {
//             format: Some(wgpu::TextureFormat::Bgra8UnormSrgb),
//             ..wgpu::TextureViewDescriptor::default()
//         });

//         let bind_groups = [
//             device.create_bind_group(&wgpu::BindGroupDescriptor {
//                 label: Some("bind group"),
//                 layout: &bind_group_layout,
//                 entries: &[
//                     wgpu::BindGroupEntry {
//                         binding: 0,
//                         resource: wgpu::BindingResource::TextureView(&texture_view_1),
//                     },
//                     wgpu::BindGroupEntry {
//                         binding: 1,
//                         resource: wgpu::BindingResource::TextureView(&texture_view_2),
//                     },
//                 ],
//             }),
//             device.create_bind_group(&wgpu::BindGroupDescriptor {
//                 label: Some("bind group"),
//                 layout: &bind_group_layout,
//                 entries: &[
//                     wgpu::BindGroupEntry {
//                         binding: 0,
//                         resource: wgpu::BindingResource::TextureView(&texture_view_2),
//                     },
//                     wgpu::BindGroupEntry {
//                         binding: 1,
//                         resource: wgpu::BindingResource::TextureView(&texture_view_1),
//                     },
//                 ],
//             }),
//         ];

//         let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
//             label: Some("Shader"),
//             source: wgpu::ShaderSource::Wgsl(include_str!("wgsl/render.wgsl").into()),
//         });
//         let render_pipeline_layout =
//             device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
//                 label: Some("Render Pipeline Layout"),
//                 bind_group_layouts: &[&bind_group_layout],
//                 push_constant_ranges: &[],
//             });
//         let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
//             label: Some("Render Pipeline"),
//             layout: Some(&render_pipeline_layout),
//             vertex: wgpu::VertexState {
//                 module: &shader,
//                 entry_point: "vert_main",
//                 buffers: &[],
//                 compilation_options: wgpu::PipelineCompilationOptions::default(),
//             },
//             fragment: Some(wgpu::FragmentState {
//                 module: &shader,
//                 entry_point: "frag_main",
//                 targets: &[Some(wgpu::ColorTargetState {
//                     format: config.format,
//                     blend: Some(wgpu::BlendState::REPLACE),
//                     write_mask: wgpu::ColorWrites::ALL,
//                 })],
//                 compilation_options: wgpu::PipelineCompilationOptions::default(),
//             }),
//             primitive: wgpu::PrimitiveState {
//                 topology: wgpu::PrimitiveTopology::TriangleList,
//                 strip_index_format: None,
//                 front_face: wgpu::FrontFace::Ccw,
//                 cull_mode: Some(wgpu::Face::Back),
//                 // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
//                 polygon_mode: wgpu::PolygonMode::Fill,
//                 // Requires Features::DEPTH_CLIP_CONTROL
//                 unclipped_depth: false,
//                 // Requires Features::CONSERVATIVE_RASTERIZATION
//                 conservative: false,
//             },
//             depth_stencil: None,
//             multisample: wgpu::MultisampleState {
//                 count: 1,
//                 mask: !0,
//                 alpha_to_coverage_enabled: false,
//             },
//             multiview: None,
//             cache: None,
//         });

//         Self {
//             window,
//             surface,
//             device,
//             queue,
//             config,
//             size,
//             render_pipeline,
//             output_textures,
//             bind_groups,
//         }
//     }

//     pub fn window(&self) -> &Window {
//         &self.window
//     }

//     pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
//         if new_size.width > 0 && new_size.height > 0 {
//             self.size = new_size;
//             self.config.width = new_size.width;
//             self.config.height = new_size.height;
//             self.surface.configure(&self.device, &self.config);
//         }
//     }

//     fn input(&mut self, event: &WindowEvent) -> bool {
//         false
//     }

//     fn update(&mut self) {}

//     fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
//         let output = self.surface.get_current_texture()?;
//         let view = output
//             .texture
//             .create_view(&wgpu::TextureViewDescriptor::default());

//         let mut encoder = self
//             .device
//             .create_command_encoder(&wgpu::CommandEncoderDescriptor {
//                 label: Some("Render Encoder"),
//             });

//         {
//             let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
//                 label: Some("Render Pass"),
//                 color_attachments: &[Some(wgpu::RenderPassColorAttachment {
//                     view: &view,
//                     resolve_target: None,
//                     ops: wgpu::Operations {
//                         load: wgpu::LoadOp::Clear(wgpu::Color {
//                             r: 0.1,
//                             g: 0.2,
//                             b: 0.3,
//                             a: 1.0,
//                         }),
//                         store: wgpu::StoreOp::Store,
//                     },
//                 })],
//                 depth_stencil_attachment: None,
//                 occlusion_query_set: None,
//                 timestamp_writes: None,
//             });

//             render_pass.set_bind_group(0, &self.bind_groups[0], &[]);
//             // Draw our 3 vertex. These are the only 3 we will need.
//             render_pass.set_pipeline(&self.render_pipeline);
//             render_pass.draw(0..3, 0..1);
//         }

//         // submit will accept anything that implements IntoIter
//         self.queue.submit(std::iter::once(encoder.finish()));
//         output.present();

//         Ok(())
//     }
// }

// fn make_render_textures(
//     device: &wgpu::Device,
//     size: &winit::dpi::PhysicalSize<u32>,
// ) -> ([wgpu::Texture; 2], [wgpu::TextureView; 2]) {
//     let textures = [
//         device.create_texture(&wgpu::TextureDescriptor {
//             label: Some("Image"),
//             size: wgpu::Extent3d {
//                 width: size.width,
//                 height: size.height,
//                 depth_or_array_layers: 1,
//             },
//             mip_level_count: 1,
//             sample_count: 1,
//             dimension: wgpu::TextureDimension::D2,
//             format: wgpu::TextureFormat::Rgba32Float,
//             usage: wgpu::TextureUsage::STORAGE
//                 | wgpu::TextureUsage::COPY_DST
//                 | wgpu::TextureUsage::COPY_SRC,
//         }),
//         device.create_texture(&wgpu::TextureDescriptor {
//             label: Some("Image"),
//             size: wgpu::Extent3d {
//                 width: size.width,
//                 height: size.height,
//                 depth_or_array_layers: 1,
//             },
//             mip_level_count: 1,
//             sample_count: 1,
//             dimension: wgpu::TextureDimension::D2,
//             format: wgpu::TextureFormat::Rgba32Float,
//             usage: wgpu::TextureUsage::STORAGE
//                 | wgpu::TextureUsage::COPY_DST
//                 | wgpu::TextureUsage::COPY_SRC,
//         }),
//     ];
//     let texture_views = [
//         textures[0].create_view(&wgpu::TextureViewDescriptor::default()),
//         textures[1].create_view(&wgpu::TextureViewDescriptor::default()),
//     ];

//     (textures, texture_views)
// }

// fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
//     self.size = new_size;
//     self.sc_desc.width = new_size.width;
//     self.sc_desc.height = new_size.height;
//     self.swap_chain = self.device.create_swap_chain(&self.surface, &self.sc_desc);

//     let new_texture_data = Self::make_render_textures(&self.device, &self.size);
//     self.render_data.render_textures = new_texture_data.0;
//     self.render_data.render_texture_views = new_texture_data.1;

//     self.render_data.render_bind_groups = [
//         self.device.create_bind_group(&wgpu::BindGroupDescriptor {
//             label: Some("render_bind_group_0"),
//             layout: &self.render_data.render_bind_group_layout,
//             entries: &[wgpu::BindGroupEntry {
//                 binding: 0,
//                 resource: wgpu::BindingResource::TextureView(
//                     &self.render_data.render_texture_views[0],
//                 ),
//             }],
//         }),
//         self.device.create_bind_group(&wgpu::BindGroupDescriptor {
//             label: Some("render_bind_group_1"),
//             layout: &self.render_data.render_bind_group_layout,
//             entries: &[wgpu::BindGroupEntry {
//                 binding: 0,
//                 resource: wgpu::BindingResource::TextureView(
//                     &self.render_data.render_texture_views[1],
//                 ),
//             }],
//         }),
//     ];

//     // self.renderer =
//     //     ProgressiveRenderer::new(self.size.width as usize, self.size.height as usize, 5);
//     self.renderer = ParallelRenderer::new(self.size.width as usize, self.size.height as usize, 5);
// }
