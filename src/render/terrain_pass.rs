use crate::{
    perf::{PHASE_RENDER_NODE_TERRAIN_PASS_CPU, PHASE_RENDER_PREPARE_DEPTH_TEXTURES, TerrainPerfTelemetry},
    shaders::{DEPTH_COPY_SHADER, DEPTH_COPY_SINGLE_SHADER},
};
use bevy::{
    core_pipeline::{FullscreenShader, core_3d::CORE_3D_DEPTH_FORMAT},
    ecs::query::QueryItem,
    prelude::*,
    render::{
        Extract,
        camera::ExtractedCamera,
        render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode},
        render_phase::{
            CachedRenderPipelinePhaseItem, DrawFunctionId, PhaseItem, PhaseItemExtraIndex,
            SortedPhaseItem, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{binding_types::{texture_depth_2d, texture_depth_2d_multisampled}, *},
        renderer::{RenderContext, RenderDevice},
        sync_world::MainEntity,
        texture::{CachedTexture, TextureCache},
        view::{RetainedViewEntity, ViewDepthTexture, ViewTarget},
    },
};
use std::ops::Range;
use std::time::Instant;

pub(crate) const TERRAIN_DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32FloatStencil8;

pub struct TerrainItem {
    pub representative_entity: (Entity, MainEntity),
    pub draw_function: DrawFunctionId,
    pub pipeline: CachedRenderPipelineId,
    pub batch_range: Range<u32>,
    pub extra_index: PhaseItemExtraIndex,
    pub order: u32,
}

impl PhaseItem for TerrainItem {
    const AUTOMATIC_BATCHING: bool = false;

    #[inline]
    fn entity(&self) -> Entity {
        self.representative_entity.0
    }

    #[inline]
    fn main_entity(&self) -> MainEntity {
        self.representative_entity.1
    }

    #[inline]
    fn draw_function(&self) -> DrawFunctionId {
        self.draw_function
    }

    #[inline]
    fn batch_range(&self) -> &Range<u32> {
        &self.batch_range
    }

    fn batch_range_mut(&mut self) -> &mut Range<u32> {
        &mut self.batch_range
    }

    fn extra_index(&self) -> PhaseItemExtraIndex {
        self.extra_index.clone()
    }

    fn batch_range_and_extra_index_mut(&mut self) -> (&mut Range<u32>, &mut PhaseItemExtraIndex) {
        (&mut self.batch_range, &mut self.extra_index)
    }
}

impl SortedPhaseItem for TerrainItem {
    type SortKey = u32;

    fn sort_key(&self) -> Self::SortKey {
        u32::MAX - self.order
    }

    fn indexed(&self) -> bool {
        false
    }
}

impl CachedRenderPipelinePhaseItem for TerrainItem {
    fn cached_pipeline(&self) -> CachedRenderPipelineId {
        self.pipeline
    }
}

pub fn extract_terrain_phases(
    mut terrain_phases: ResMut<ViewSortedRenderPhases<TerrainItem>>,
    cameras: Extract<Query<(Entity, &Camera), With<Camera3d>>>,
) {
    terrain_phases.clear();

    for (entity, camera) in &cameras {
        if !camera.is_active {
            continue;
        }

        terrain_phases.insert(
            RetainedViewEntity {
                main_entity: entity.into(),
                auxiliary_entity: Entity::PLACEHOLDER.into(),
                subview_index: 0,
            },
            default(),
        );
    }
}

#[derive(Component)]
pub struct TerrainViewDepthTexture {
    texture: Texture,
    pub view: TextureView,
    pub depth_view: TextureView,
    pub stencil_view: TextureView,
}

impl TerrainViewDepthTexture {
    pub fn new(texture: CachedTexture) -> Self {
        let depth_view = texture.texture.create_view(&TextureViewDescriptor {
            aspect: TextureAspect::DepthOnly,
            ..default()
        });
        let stencil_view = texture.texture.create_view(&TextureViewDescriptor {
            aspect: TextureAspect::StencilOnly,
            ..default()
        });

        Self {
            texture: texture.texture,
            view: texture.default_view,
            depth_view,
            stencil_view,
        }
    }

    pub fn get_attachment(&self) -> RenderPassDepthStencilAttachment<'_> {
        RenderPassDepthStencilAttachment {
            view: &self.view,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(0.0), // Clear depth
                store: StoreOp::Store,
            }),
            stencil_ops: Some(Operations {
                load: LoadOp::Clear(0), // Initialize stencil to 0 (lowest priority)
                store: StoreOp::Store,
            }),
        }
    }
}

pub fn prepare_terrain_depth_textures(
    mut commands: Commands,
    mut texture_cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views_3d: Query<(Entity, &ExtractedCamera, &Msaa)>,
    perf_telemetry: Res<TerrainPerfTelemetry>,
) {
    let start = Instant::now();
    for (view, camera, msaa) in &views_3d {
        let Some(physical_target_size) = camera.physical_target_size else {
            continue;
        };

        let descriptor = TextureDescriptor {
            label: Some("view_depth_texture"),
            size: Extent3d {
                depth_or_array_layers: 1,
                width: physical_target_size.x,
                height: physical_target_size.y,
            },
            mip_level_count: 1,
            sample_count: msaa.samples(),
            dimension: TextureDimension::D2,
            format: TERRAIN_DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        let cached_texture = texture_cache.get(&device, descriptor);

        commands
            .entity(view)
            .insert(TerrainViewDepthTexture::new(cached_texture));
    }
    perf_telemetry.record_duration(PHASE_RENDER_PREPARE_DEPTH_TEXTURES, start.elapsed());
}

#[derive(Resource)]
pub struct DepthCopyPipeline {
    single_sample_layout: BindGroupLayout,
    multisampled_layout: BindGroupLayout,
    single_sample_id: CachedRenderPipelineId,
    sample2_id: CachedRenderPipelineId,
    sample4_id: CachedRenderPipelineId,
}

impl DepthCopyPipeline {
    fn pipeline_and_layout(&self, sample_count: u32) -> (CachedRenderPipelineId, &BindGroupLayout) {
        match sample_count {
            1 => (self.single_sample_id, &self.single_sample_layout),
            2 => (self.sample2_id, &self.multisampled_layout),
            4 => (self.sample4_id, &self.multisampled_layout),
            _ => panic!("Unsupported depth copy sample count: {sample_count}"),
        }
    }
}

impl FromWorld for DepthCopyPipeline {
    fn from_world(world: &mut World) -> Self {
        let pipeline_cache = world.resource::<PipelineCache>();
        let fullscreen_shader = world.resource::<FullscreenShader>();

        let single_sample_layout_descriptor = BindGroupLayoutDescriptor::new(
            "depth_copy_single_pipeline_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_depth_2d(),)),
        );
        let single_sample_layout =
            pipeline_cache.get_bind_group_layout(&single_sample_layout_descriptor);

        let multisampled_layout_descriptor = BindGroupLayoutDescriptor::new(
            "depth_copy_multisampled_pipeline_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (texture_depth_2d_multisampled(),),
            ),
        );
        let multisampled_layout =
            pipeline_cache.get_bind_group_layout(&multisampled_layout_descriptor);

        let queue_pipeline = |label: &'static str,
                              shader: Handle<Shader>,
                              layout: BindGroupLayoutDescriptor,
                              sample_count: u32| {
            pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(label.into()),
                layout: vec![layout],
                push_constant_ranges: Vec::new(),
                vertex: fullscreen_shader.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader,
                    shader_defs: vec![],
                    entry_point: Some("fragment".into()),
                    targets: vec![],
                }),
                primitive: Default::default(),
                depth_stencil: Some(DepthStencilState {
                    format: CORE_3D_DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: CompareFunction::Always,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: MultisampleState {
                    count: sample_count,
                    ..Default::default()
                },
                zero_initialize_workgroup_memory: false,
            })
        };

        let single_sample_id = queue_pipeline(
            "depth_copy_single_pipeline",
            world.load_asset(DEPTH_COPY_SINGLE_SHADER),
            single_sample_layout_descriptor,
            1,
        );
        let sample2_id = queue_pipeline(
            "depth_copy_msaa2_pipeline",
            world.load_asset(DEPTH_COPY_SHADER),
            multisampled_layout_descriptor.clone(),
            2,
        );
        let sample4_id = queue_pipeline(
            "depth_copy_msaa4_pipeline",
            world.load_asset(DEPTH_COPY_SHADER),
            multisampled_layout_descriptor.clone(),
            4,
        );
        Self {
            single_sample_layout,
            multisampled_layout,
            single_sample_id,
            sample2_id,
            sample4_id,
        }
    }
}

#[derive(Debug, Hash, Default, PartialEq, Eq, Clone, RenderLabel)]
pub struct TerrainPass;

impl ViewNode for TerrainPass {
    type ViewQuery = (
        Entity,
        MainEntity,
        &'static ExtractedCamera,
        &'static Msaa,
        &'static ViewTarget,
        &'static ViewDepthTexture,
        &'static TerrainViewDepthTexture,
    );

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (render_view, main_view, camera, msaa, target, depth, terrain_depth): QueryItem<
            'w,
            '_,
            Self::ViewQuery,
        >,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let device = world.resource::<RenderDevice>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let depth_copy_pipeline = world.resource::<DepthCopyPipeline>();
        let perf_telemetry = world.resource::<TerrainPerfTelemetry>().clone();

        let (depth_copy_pipeline_id, depth_copy_layout) =
            depth_copy_pipeline.pipeline_and_layout(msaa.samples());
        let Some(pipeline) = pipeline_cache.get_render_pipeline(depth_copy_pipeline_id) else {
            return Ok(());
        };

        let Some(terrain_phase) = world
            .get_resource::<ViewSortedRenderPhases<TerrainItem>>()
            .and_then(|phase| {
                phase.get(&RetainedViewEntity {
                    main_entity: main_view.into(),
                    auxiliary_entity: Entity::PLACEHOLDER.into(),
                    subview_index: 0,
                })
            })
        else {
            return Ok(());
        };

        if terrain_phase.items.is_empty() {
            return Ok(());
        }

        // Todo: prepare this in a separate system
        let terrain_depth_view = terrain_depth.texture.create_view(&TextureViewDescriptor {
            aspect: TextureAspect::DepthOnly,
            ..default()
        });
        let depth_copy_bind_group = device.create_bind_group(
            None,
            depth_copy_layout,
            &BindGroupEntries::single(&terrain_depth_view),
        );

        // call this here, otherwise the order between passes is incorrect
        let color_attachments = [Some(target.get_color_attachment())];
        let terrain_depth_stencil_attachment = Some(terrain_depth.get_attachment());
        let depth_stencil_attachment = Some(depth.get_attachment(StoreOp::Store));

        context.add_command_buffer_generation_task(move |device| {
            let start = Instant::now();
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());

            let pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("terrain_pass"),
                color_attachments: &color_attachments,
                depth_stencil_attachment: terrain_depth_stencil_attachment,
                ..default()
            });
            let mut pass = TrackedRenderPass::new(&device, pass);

            if let Some(viewport) = camera.viewport.as_ref() {
                pass.set_camera_viewport(viewport);
            }

            terrain_phase.render(&mut pass, world, render_view).unwrap();
            drop(pass);

            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                depth_stencil_attachment,
                ..default()
            });
            pass.set_bind_group(0, &depth_copy_bind_group, &[]);
            pass.set_pipeline(pipeline);
            pass.draw(0..3, 0..1);
            drop(pass);

            perf_telemetry.record_duration(PHASE_RENDER_NODE_TERRAIN_PASS_CPU, start.elapsed());
            encoder.finish()
        });

        Ok(())
    }
}
