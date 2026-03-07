use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems, extract_resource::ExtractResource,
        extract_resource::ExtractResourcePlugin, renderer::RenderDevice,
    },
};
use std::{env::current_dir, fs, path::PathBuf, time::SystemTime};

pub struct MetalCapturePlugin;

impl Plugin for MetalCapturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameCapture>()
            .add_plugins(ExtractResourcePlugin::<FrameCapture>::default())
            .add_systems(Update, input_capture);

        app.sub_app_mut(RenderApp)
            .add_systems(Render, start_capture.in_set(RenderSystems::Prepare))
            .add_systems(Render, stop_capture.in_set(RenderSystems::Cleanup));
    }
}

#[derive(Clone, Default, Resource, ExtractResource)]
pub struct FrameCapture {
    pub capture: bool,
    pub output_dir: Option<PathBuf>,
    pub label: Option<String>,
}

pub fn input_capture(input: Res<ButtonInput<KeyCode>>, mut capture: ResMut<FrameCapture>) {
    capture.capture = input.just_pressed(KeyCode::KeyC);
}

pub fn start_capture(capture: Res<FrameCapture>, device: Res<RenderDevice>) {
    if !capture.capture {
        return;
    }

    println!("Capturing frame");

    let output_dir = capture
        .output_dir
        .clone()
        .unwrap_or_else(|| current_dir().unwrap().join("captures"));
    let _ = fs::create_dir_all(&output_dir);
    let label = capture
        .label
        .clone()
        .unwrap_or_else(|| "capture".to_string())
        .replace(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_', "_");

    let capture = metal::CaptureDescriptor::new();
    capture.set_destination(metal::MTLCaptureDestination::GpuTraceDocument);
    capture.set_output_url(output_dir.join(format!(
            "{}_{}.gputrace",
            label,
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        )));
    let Some(device) = (unsafe { device.wgpu_device().as_hal::<wgpu_hal::api::Metal>() }) else {
        println!("Failed to access Metal device for capture");
        return;
    };
    let raw_device = device.raw_device().lock();
    capture.set_capture_device(raw_device.as_ref());

    let manager = metal::CaptureManager::shared();
    if !manager.supports_destination(metal::MTLCaptureDestination::GpuTraceDocument) {
        println!("Metal capture destination GpuTraceDocument is not supported");
        return;
    }

    if let Err(err) = manager.start_capture(&capture) {
        println!("Failed to start capture: {err}");
    }
}

pub fn stop_capture(capture: Res<FrameCapture>) {
    if !capture.capture {
        return;
    }

    metal::CaptureManager::shared().stop_capture();
}
