use crate::{
    formats::TiffLoader,
    perf::TerrainPerfTelemetry,
    preprocess::{MipPipelines, MipPrepass},
    render::{
        DepthCopyPipeline, GpuTerrain, GpuTerrainView, TerrainItem, TerrainPass,
        TerrainTilingPrepassPipelines, TilingPrepass, TilingPrepassItem, extract_terrain_phases,
        prepare_terrain_depth_textures, queue_tiling_prepass,
    },
    shaders::{InternalShaders, load_terrain_shaders},
    streaming::{
        NasaGibsImageryProvider, OpenTopographyHeightProvider, StreamingRequestQueue,
        StreamingWorker, TerrainStreamingSettings, collect_streaming_requests,
        finish_streaming_jobs, start_streaming_jobs,
    },
    terrain::{TerrainComponents, TerrainConfig},
    terrain_data::{
        AttachmentLabel, GpuTileAtlas, TileAtlas, TileTree, finish_loading, start_loading,
    },
    terrain_view::TerrainViewComponents,
};
use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        graph::CameraDriverLabel,
        render_graph::{RenderGraph, RenderGraphExt, ViewNodeRunner},
        render_phase::{DrawFunctions, ViewSortedRenderPhases, sort_phase_system},
        render_resource::*,
    },
};
use bevy_common_assets::ron::RonAssetPlugin;
use big_space::prelude::*;

#[derive(Resource)]
pub struct TerrainSettings {
    pub attachments: Vec<AttachmentLabel>,
    pub atlas_size: u32,
    pub upload_budget_bytes_per_frame: usize,
    pub streaming_cache_root: Option<String>,
    pub streaming_target_lod_count: Option<u32>,
}

impl Default for TerrainSettings {
    fn default() -> Self {
        Self {
            attachments: vec![AttachmentLabel::Height],
            atlas_size: 1028,
            upload_budget_bytes_per_frame: 24 * 1024 * 1024,
            streaming_cache_root: None,
            streaming_target_lod_count: None,
        }
    }
}

impl TerrainSettings {
    /// Create settings with height plus any custom attachments you want the renderer to stream.
    pub fn new<I, S>(custom_attachments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut attachments = vec![AttachmentLabel::Height];
        attachments.extend(
            custom_attachments
                .into_iter()
                .map(|name| AttachmentLabel::Custom(name.as_ref().to_string())),
        );

        Self {
            attachments,
            atlas_size: 1028,
            upload_budget_bytes_per_frame: 24 * 1024 * 1024,
            streaming_cache_root: None,
            streaming_target_lod_count: None,
        }
    }

    /// Stream height plus a single custom attachment.
    pub fn with_attachment<S: AsRef<str>>(attachment: S) -> Self {
        Self::new([attachment.as_ref()])
    }

    /// Stream height plus the conventional `albedo` attachment.
    pub fn with_albedo() -> Self {
        Self::with_attachment("albedo")
    }

    pub fn with_upload_budget_bytes_per_frame(
        mut self,
        upload_budget_bytes_per_frame: usize,
    ) -> Self {
        self.upload_budget_bytes_per_frame = upload_budget_bytes_per_frame;
        self
    }

    /// Prefer cached streamed tiles from an asset-relative root before the bundled starter data.
    pub fn with_streaming_cache_root<S: Into<String>>(mut self, streaming_cache_root: S) -> Self {
        self.streaming_cache_root = Some(streaming_cache_root.into());
        self
    }

    /// Allow the runtime to request higher LODs than the bundled starter dataset contains.
    /// This is primarily for online cache-fill scenarios where missing child tiles can be
    /// streamed and cached while the renderer falls back to parent LODs meanwhile.
    pub fn with_streaming_target_lod_count(mut self, lod_count: u32) -> Self {
        self.streaming_target_lod_count = Some(lod_count);
        self
    }

    pub fn effective_terrain_lod_count(&self, config_lod_count: u32) -> u32 {
        self.streaming_target_lod_count
            .map(|override_lod| override_lod.max(config_lod_count))
            .unwrap_or(config_lod_count)
    }
}

/// The plugin for the terrain renderer.
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        let perf_telemetry = TerrainPerfTelemetry::default();
        app.insert_resource(perf_telemetry.clone());
        app.add_plugins(BigSpaceDefaultPlugins);

        app.add_plugins(RonAssetPlugin::<TerrainConfig>::new(&["tc.ron"]))
            .init_asset::<TerrainConfig>()
            .init_resource::<InternalShaders>()
            .init_resource::<TerrainViewComponents<TileTree>>()
            .init_resource::<TerrainSettings>()
            .init_resource::<NasaGibsImageryProvider>()
            .init_resource::<OpenTopographyHeightProvider>()
            .init_resource::<TerrainStreamingSettings>()
            .init_resource::<StreamingRequestQueue>()
            .init_resource::<StreamingWorker>()
            .init_asset_loader::<TiffLoader>()
            .add_systems(
                PostUpdate,
                (
                    // Todo: enable visibility checking again
                    // check_visibility::<With<TileAtlas>>.in_set(VisibilitySystems::CheckVisibility),
                    (
                        TileTree::compute_requests,
                        finish_loading,
                        TileAtlas::update,
                        collect_streaming_requests,
                        finish_streaming_jobs,
                        start_streaming_jobs,
                        start_loading,
                        TileTree::adjust_to_tile_atlas,
                        TileTree::generate_surface_approximation,
                        TileTree::update_terrain_view_buffer,
                        TileAtlas::update_terrain_buffer,
                    )
                        .chain()
                        .after(TransformSystems::Propagate),
                ),
            );
        app.sub_app_mut(RenderApp)
            .insert_resource(perf_telemetry)
            .init_resource::<SpecializedComputePipelines<MipPipelines>>()
            .init_resource::<SpecializedComputePipelines<TerrainTilingPrepassPipelines>>()
            .init_resource::<TerrainComponents<GpuTileAtlas>>()
            .init_resource::<TerrainComponents<GpuTerrain>>()
            .init_resource::<TerrainViewComponents<GpuTerrainView>>()
            .init_resource::<TerrainViewComponents<TilingPrepassItem>>()
            .init_resource::<DrawFunctions<TerrainItem>>()
            .init_resource::<ViewSortedRenderPhases<TerrainItem>>()
            .add_systems(
                ExtractSchedule,
                (
                    extract_terrain_phases,
                    GpuTileAtlas::initialize,
                    GpuTileAtlas::extract.after(GpuTileAtlas::initialize),
                    GpuTerrain::initialize.after(GpuTileAtlas::initialize),
                    GpuTerrainView::initialize,
                ),
            )
            .add_systems(
                Render,
                (
                    (
                        GpuTileAtlas::prepare,
                        GpuTerrain::prepare,
                        GpuTerrainView::prepare_terrain_view,
                        GpuTerrainView::prepare_indirect,
                        GpuTerrainView::prepare_refine_tiles,
                    )
                        .in_set(RenderSystems::Prepare),
                    sort_phase_system::<TerrainItem>.in_set(RenderSystems::PhaseSort),
                    prepare_terrain_depth_textures.in_set(RenderSystems::PrepareResources),
                    (queue_tiling_prepass, GpuTileAtlas::queue).in_set(RenderSystems::Queue),
                    GpuTileAtlas::_cleanup
                        .before(World::clear_entities)
                        .in_set(RenderSystems::Cleanup),
                ),
            )
            .add_render_graph_node::<ViewNodeRunner<TerrainPass>>(Core3d, TerrainPass)
            .add_render_graph_edges(
                Core3d,
                (Node3d::StartMainPass, TerrainPass, Node3d::MainOpaquePass),
            );

        let mut render_graph = app
            .sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<RenderGraph>();
        render_graph.add_node(MipPrepass, MipPrepass);
        render_graph.add_node(TilingPrepass, TilingPrepass);
        render_graph.add_node_edge(MipPrepass, TilingPrepass);
        render_graph.add_node_edge(TilingPrepass, CameraDriverLabel);
    }

    fn finish(&self, app: &mut App) {
        let attachments = app
            .world()
            .resource::<TerrainSettings>()
            .attachments
            .clone();

        load_terrain_shaders(app, &attachments);

        app.sub_app_mut(RenderApp)
            .init_resource::<TerrainTilingPrepassPipelines>()
            .init_resource::<MipPipelines>()
            .init_resource::<DepthCopyPipeline>();
    }
}
