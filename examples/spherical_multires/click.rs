// Click inspection HUD, marker placement, and clipboard support for the multires demo.

use super::*;

pub fn inspect_clicked_terrain_point(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mode: Res<RuntimeMode>,
    grids: Grids,
    cameras: Query<(Entity, &PickingData), With<PrimaryTerrainCamera>>,
    mut click_markers: Query<(&mut Transform, &mut CellCoord, &mut Visibility), With<ClickMarker>>,
    mut click_readout: ResMut<ClickReadoutState>,
) {
    if !mode.click_readout_enabled || !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }

    match cameras.single() {
        Ok((entity, picking_data)) => match (picking_data.translation, grids.parent_grid(entity)) {
            (Some(hit_translation), Some(grid)) => {
                let local_position = grid.grid_position_double(
                    &picking_data.cell,
                    &Transform::from_translation(hit_translation),
                );
                let marker_position =
                    local_position + local_position.normalize_or_zero() * CLICK_MARKER_OFFSET_M;
                let lla = renderer_local_to_lla_hae(local_position);

                if let Ok((mut marker_transform, mut marker_cell, mut marker_visibility)) =
                    click_markers.single_mut()
                {
                    let (new_cell, new_translation) = grid.translation_to_grid(marker_position);
                    *marker_cell = new_cell;
                    marker_transform.translation = new_translation;
                    *marker_visibility = Visibility::Visible;
                }

                info!(
                    target: "click",
                    "terrain_click lat_deg={:.8} lon_deg={:.8} hae_m={:.3} local_x_m={:.3} local_y_m={:.3} local_z_m={:.3}",
                    lla.lat_deg,
                    lla.lon_deg,
                    lla.hae_m,
                    local_position.x,
                    local_position.y,
                    local_position.z
                );

                click_readout.summary_line = format!(
                    "Lat {:.8} deg | Lon {:.8} deg | WGS84 HAE {:.3} m",
                    lla.lat_deg, lla.lon_deg, lla.hae_m
                );
                click_readout.detail_line = format!(
                    "Renderer local XYZ = ({:.3}, {:.3}, {:.3}) m",
                    local_position.x, local_position.y, local_position.z
                );
                click_readout.status_line = CLICK_COPY_PROMPT.to_string();
                click_readout.clipboard_payload = Some(format!(
                    concat!(
                        "lat_deg={:.8}\n",
                        "lon_deg={:.8}\n",
                        "wgs84_hae_m={:.3}\n",
                        "renderer_local_x_m={:.3}\n",
                        "renderer_local_y_m={:.3}\n",
                        "renderer_local_z_m={:.3}\n"
                    ),
                    lla.lat_deg,
                    lla.lon_deg,
                    lla.hae_m,
                    local_position.x,
                    local_position.y,
                    local_position.z
                ));
            }
            (None, _) => {
                if let Ok((_, _, mut marker_visibility)) = click_markers.single_mut() {
                    *marker_visibility = Visibility::Hidden;
                }
                click_readout.summary_line = "No terrain hit under cursor.".to_string();
                click_readout.detail_line =
                    "Renderer local XYZ is only available for valid terrain hits.".to_string();
                click_readout.status_line = CLICK_COPY_PROMPT.to_string();
            }
            (_, None) => {
                if let Ok((_, _, mut marker_visibility)) = click_markers.single_mut() {
                    *marker_visibility = Visibility::Hidden;
                }
                click_readout.summary_line =
                    "No terrain grid available for click inspection.".to_string();
                click_readout.detail_line =
                    "Renderer local XYZ is only available for valid terrain hits.".to_string();
                click_readout.status_line = CLICK_COPY_PROMPT.to_string();
            }
        },
        Err(_) => {
            if let Ok((_, _, mut marker_visibility)) = click_markers.single_mut() {
                *marker_visibility = Visibility::Hidden;
            }
            click_readout.summary_line =
                "No primary terrain camera available for click inspection.".to_string();
            click_readout.detail_line =
                "Renderer local XYZ is only available for valid terrain hits.".to_string();
            click_readout.status_line = CLICK_COPY_PROMPT.to_string();
        }
    }
}

pub fn copy_click_readout_to_clipboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<RuntimeMode>,
    mut click_readout: ResMut<ClickReadoutState>,
) {
    if !mode.click_readout_enabled || !copy_shortcut_pressed(&keyboard) {
        return;
    }

    let Some(payload) = click_readout.clipboard_payload.clone() else {
        click_readout.status_line = "No clicked coordinates available to copy yet.".to_string();
        return;
    };

    match copy_text_to_clipboard(&payload) {
        Ok(()) => {
            click_readout.status_line = "Copied last clicked coordinates to clipboard.".to_string();
        }
        Err(error) => {
            click_readout.status_line = format!("Clipboard copy failed: {error}");
            warn!(target: "click", "clipboard copy failed: {error}");
        }
    }
}

pub fn update_click_readout_ui(
    mode: Res<RuntimeMode>,
    click_readout: Res<ClickReadoutState>,
    mut readout_text: Query<&mut Text, With<ClickReadoutText>>,
) {
    if !mode.click_readout_enabled || !click_readout.is_changed() {
        return;
    }

    for mut text in &mut readout_text {
        *text = Text::new(click_readout.text());
    }
}

fn copy_shortcut_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.just_pressed(KeyCode::KeyC)
        && (keyboard.pressed(KeyCode::SuperLeft)
            || keyboard.pressed(KeyCode::SuperRight)
            || keyboard.pressed(KeyCode::ControlLeft)
            || keyboard.pressed(KeyCode::ControlRight))
}

fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut child = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to spawn pbcopy: {error}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to open pbcopy stdin".to_string())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("failed to write pbcopy stdin: {error}"))?;
        drop(stdin);
        let status = child
            .wait()
            .map_err(|error| format!("failed to wait for pbcopy: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("pbcopy exited with status {status}"))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Err("clipboard copy is only implemented for macOS in this demo".to_string())
    }
}
