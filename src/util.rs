use std::{io::{Read, Write}};

use serialport::SerialPortType;
use vexv5_serial::{ports::{VexSerialInfo, VexSerialClass}, device::V5FileHandle};
use console::style;
use dialoguer::{
    Select,
    theme::ColorfulTheme
};
use indicatif::{ProgressBar, ProgressStyle};
use anyhow::Result;



#[derive(Clone, Debug)]
pub enum DevicePair {
    Single(VexSerialInfo),
    Double(VexSerialInfo, VexSerialInfo)
}


pub fn find_devices() -> Result<DevicePair> {
    // Try to find vex devices
    let devices = vexv5_serial::ports::discover_vex_ports()?;

    // Create an empty vector with device pairs
    let mut pairs = Vec::<DevicePair>::new();

    // This mimics behavior of PROS assuming that the second device is always the user device.
    for device in devices {
        match device.class {
            VexSerialClass::User => {
                if pairs.len() > 0 {
                    
                    if let DevicePair::Single(d) = pairs.last().unwrap().clone() {
                        pairs.pop();
                        pairs.push(DevicePair::Double(d.clone(), device));
                    } else {
                        pairs.push(DevicePair::Single(device));
                    }
                } else {
                    pairs.push(DevicePair::Single(device));
                }
            },
            _ => {
                pairs.push(DevicePair::Single(device));
            },
            
        }
    }
    
    // If there are no devices, then error
    if pairs.len() == 0 {
        print!("{} ", style("Error:").red().bright());
        println!("{}", style("No Vex devices found.").black().bright());
        return Err(anyhow::anyhow!("No Devices Found"));
    }

    // If there is only one device, then use it.
    // If not, then ask which one to use
    let device = if pairs.len() == 1 {
        pairs[0].clone()
    } else {
        let device = pairs[0].clone();

        // Generate a list of selections (just differently formatted devices)
        let mut pselect = Vec::<String>::new();
        for pair in pairs.clone() {
            if let DevicePair::Single(d1) = pair {
                pselect.push(format!("{:?} port: {} ({})", d1.class, d1.port_info.port_name, match d1.port_info.port_type {
                    SerialPortType::UsbPort(p) => {
                        p.product.unwrap_or("".to_string())
                    },
                    _ => {
                        "Unsupported Device".to_string()
                    }
                }));
            } else if let DevicePair::Double(d1, d2) = pair {
                pselect.push(format!("Vex Brain with ports {} and {}",
                    d1.port_info.port_name,
                    d2.port_info.port_name
                ));
            }
            
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .items(&pselect)
            .default(0)
            .with_prompt("Multiple Vex devices found. Please select which one to use:")
            .interact()?;

        pairs[selection].clone()
    };

    Ok(device)
}

/// Writes a vector up to the file length of data to the file. 
/// Ignores any extra bytes at the end of the vector.
/// Returns the ammount of data read
/// Same as the function provided in vexv5_serial but it shows progress to the user.
pub fn write_file_progress<T: Read + Write>(handle: &mut V5FileHandle<T>, data: Vec<u8>) -> Result<usize> {

    // Save the max size so it is easier to access
    // We want it to be 3/4 size so we do not have issues with packet headers
    // going over the max size
    let max_size = handle.transfer_metadata.max_packet_size / 
    2 + (handle.transfer_metadata.max_packet_size / 4);
    
    // We will be using the length of the file in the metadata
    // that way we do not ever write more data than is expected.
    // However, if the vector is smaller than the file size
    // Then use the vector size.
    let size = if data.len() as u32 > handle.transfer_metadata.file_size {
        handle.transfer_metadata.file_size
    } else {
        data.len() as u32
    };

    

    // We will be incrementing this variable so we know how much we have written
    let mut how_much: usize = 0;
    
    // Create the progress bar
    let bar = ProgressBar::new(size.into());

    // Style the progress bar
    bar.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {binary_bytes_per_sec} {bar:40.cyan/blue} {percent}% {bytes:>7}/{total_bytes:7} {msg}")
        .progress_chars("##-"));

    // Iterate over the file's length in steps of max_size
    // We will be writing each iteration.
    for i in (0..size as usize).step_by(max_size.into()) {
        // Determine the packet size. We do not want to write
        // max_size bytes if we are at the end of the file
        let packet_size = if size < max_size as u32 {
            size as u16
        } else if i as u32 + max_size as u32 > size {
            (size - i as u32) as u16
        } else {
            max_size
        };

        // Cut out packet_size bytes out of the provided buffer
        let payload = data[i..i+packet_size as usize].to_vec();

        // Write the payload to the file
        handle.write_some(handle.metadata.addr + i as u32, payload)?;

        // Update the progress bar
        bar.inc(packet_size.into());

        // Increment how_much by packet data so we know how much we
        // have written to the file
        how_much += packet_size as usize;
    }

    // Finalize the progress bar
    bar.finish();

    Ok(how_much)
}