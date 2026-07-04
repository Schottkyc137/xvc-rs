use std::fmt;

use inquire::{InquireError, Select, ui::RenderConfig};

use crate::ftdi_device::FtdiJtagDevice;

pub fn disambiguate_available_devices(
    mut available: Vec<FtdiJtagDevice>,
    interactive: bool,
) -> Option<FtdiJtagDevice> {
    match available.len() {
        0 => {
            log::error!(
                "No FTDI device found. Make sure the adapter is plugged in and that you have \
                 permission to access it (on Linux, check your udev rules)."
            );
            None
        }
        1 => available.pop(),
        len if !interactive => {
            let listing = available
                .iter()
                .map(|device| format!("- {}", device.info()))
                .collect::<Vec<_>>()
                .join("\n");
            log::error!("Found {len} matching FTDI devices:\n{listing}");
            None
        }
        _ => prompt_device_selection(available),
    }
}

/// A selectable device: the aligned, human-readable label shown in the prompt.
struct DeviceChoice {
    label: String,
    device: FtdiJtagDevice,
}

impl fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// One line per device with columns padded so equal fields line up vertically,
/// making the differences (serial, bus position) easy to spot.
fn device_labels(devices: &[FtdiJtagDevice]) -> Vec<String> {
    let rows: Vec<_> = devices
        .iter()
        .map(|device| {
            let info = device.info();
            [
                info.product
                    .clone()
                    .unwrap_or_else(|| "Unknown product".to_owned()),
                info.chip_model().unwrap_or("unknown chip").to_owned(),
                info.serial
                    .as_ref()
                    .map(|serial| format!("serial {serial}"))
                    .unwrap_or_else(|| "no serial".to_owned()),
                format!("bus {:03} device {:03}", info.bus_number, info.address),
            ]
        })
        .collect();

    let widths: [_; 4] = std::array::from_fn(|col| {
        rows.iter()
            .map(|row| row[col].chars().count())
            .max()
            .unwrap_or(0)
    });

    rows.iter()
        .map(|row| {
            row.iter()
                .zip(widths)
                .map(|(cell, width)| format!("{cell:<width$}"))
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn select_render_config() -> RenderConfig<'static> {
    RenderConfig::default()
}

fn prompt_device_selection(available: Vec<FtdiJtagDevice>) -> Option<FtdiJtagDevice> {
    let labels = device_labels(&available);
    let choices: Vec<DeviceChoice> = labels
        .into_iter()
        .zip(available)
        .map(|(label, device)| DeviceChoice { label, device })
        .collect();

    let prompt = format!("Found {} matching FTDI devices", choices.len());
    match Select::new(&prompt, choices)
        .with_help_message("↑↓ to move, enter to select, type to filter, esc to abort")
        .with_render_config(select_render_config())
        .prompt()
    {
        Ok(choice) => Some(choice.device),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            log::info!("Device selection aborted");
            None
        }
        Err(err) => {
            log::error!("Device selection failed: {err}");
            None
        }
    }
}
