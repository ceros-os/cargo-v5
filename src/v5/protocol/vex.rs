use std::io::{Read, Write};
use anyhow::{Result, anyhow};
use num_derive::FromPrimitive;
use std::time::{Duration, SystemTime};
use crc::Algorithm;


const VEX_CRC16: Algorithm<u16> = Algorithm {
    poly: 0x1021,
    init: 0xffff,
    refin: false,
    refout: false,
    xorout: 0x0000,
    check: 0xe5cc,
    residue: 0x0000,
};

/// Represents the type of a vex device
pub enum VexDeviceType {
    User,
    System,
    Joystick,
    Unknown
}


/// Represents a vex device command
#[repr(u8)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum VexDeviceCommand {
    ExecuteFile = 0x18,
    Extended = 0x56,
}



impl PartialEq<u8> for VexDeviceCommand {
    fn eq(&self, other: &u8) -> bool {
        *self as u8 == *other
    }
}

impl PartialEq<VexDeviceCommand> for u8 {
    fn eq(&self, other: &VexDeviceCommand) -> bool {
        *self == *other as u8
    }
}


/// Wraps any struct that implements both read and write
/// traits. Allows sending vex device commands. 
pub struct VexProtocolWrapper<T> 
    where T: Read + Write {
    device_type: VexDeviceType,
    wraps: T
}

impl<T> VexProtocolWrapper<T> 
    where T: Read + Write {

    /// Initializes a new VexProtocolWrapper
    pub fn new(device_type: VexDeviceType, wraps: T) -> VexProtocolWrapper<T> {
        VexProtocolWrapper {
            device_type,
            wraps
        }
    }

    /// Sends an extended packet to the vex device
    pub fn send_extended(&mut self, command: VexDeviceCommand, data: Vec<u8>) -> Result<usize> {
        
        // Create the payload
        let payload = self.create_extended_packet(command, data)?;
        
        // Send the payload and return the length of the data sent
        self.send_simple(VexDeviceCommand::Extended, payload)
    }
    

    /// Receives a simple packet from the vex device
    pub fn receive_simple(&mut self, timeout: Option<Duration>) -> Result<(VexDeviceCommand, Vec<u8>)> {

        // Use default timeout if none was provided
        let timeout = match timeout {
            Some(t) => t,
            None => Duration::new(0,100000000)
        };


        // This is the expected header in the response:
        let expected_header: [u8;2] = [0xAA,0x55];
        let mut header_index = 0;

        // Set the duration for the next timeout
        let then = SystemTime::now() + timeout;

        // Iterate over recieving single bytes untill we recieve the header
        while header_index < 3 {
            // Recieve a single byte
            let mut b: [u8; 1] = [0];
            self.wraps.read_exact(&mut b)?;

            // If the byte is equivilent to the current index in expected header
            // then increment the current index. if not, then set it back to zero
            if b[0] == expected_header[header_index] {
                header_index += 1;
            } else {
                header_index = 0;
            }

            // If the timeout is elapsed then return an error
            if !then.elapsed().unwrap_or(Duration::new(0, 0)).is_zero() && header_index < 3 {
                return Err(anyhow!("Unable to find response header in timeout, so unable to recieve data from device."));
            }
        }

        // Read in the next two bytes
        let mut buf: [u8; 2] = [0, 0];
        self.wraps.read_exact(&mut buf)?;

        // Extract the command and the length of the packet
        let command = buf[0];
        let mut length: u16 = buf[1].into();
        
        // If this is an extended command
        if command == VexDeviceCommand::Extended {
            // Then extract the lower byte of the length
            let mut b: [u8; 1] = [0];
            self.wraps.read_exact(&mut b)?;

            let b: u16 = b[0].into();

            // And append it to the length
            length <<= 8;
            length |= b;
        }

        // Read in the rest of the payload
        let mut payload: Vec<u8> = vec![0; length.into()];
        self.wraps.read(&mut payload)?; // We do not care about size here. Some commands may send less data than needed.

        // Try to convert the command into it's enum format
        let command: VexDeviceCommand =  match num::FromPrimitive::from_u8(command) {
            Some(c) => c,
            None => {
                return Err(anyhow!("Unknown command {}", command));
            }
        };
        
        // Return the command and the payload
        Ok((command, payload))
    }

    /// Sends a simple packet to the vex device
    pub fn send_simple(&mut self, command: VexDeviceCommand, data: Vec<u8>) -> Result<usize>{

        // Create the packet
        let mut packet = self.create_packet(command);

        // Add the data to the packet
        packet.append(&mut data.clone());
        
        
        println!("{:?}", packet);
        // Write the data
        self.wraps.write_all(&mut packet)?;
        

        // Flush all pending writes on the buffer.
        self.wraps.flush()?;
        
        
        // Return the length of the data sent
        Ok(packet.len())
    }

    /// Creates a simple packet with a magic number
    /// and a command message for a vex device.
    fn create_packet(&self, msg: VexDeviceCommand) -> Vec<u8> {
        // Create a vec of bytes containing the magic number (or at least we assume it is)
        // and the command code.
        vec![0xc9, 0x36, 0xb8, 0x47, msg as u8]
    }

    /// Creates an extended packet
    fn create_extended_packet(&self, msg: VexDeviceCommand, payload: Vec<u8>) -> Result<Vec<u8>> {

        let mut packet = Vec::<u8>::new();

        packet.push(msg as u8);

        // Cache this value because it will not change.
        let payload_length: u8 = payload.len().try_into()?;

        // If we are larger than an 8-bit signed integer
        // then split the length into two halves
        if payload_length > 0x80 {
            packet.push((payload_length.checked_shr(8).unwrap_or(0)) | 0x80);
            packet.push(payload_length & 0xff);
        }

        // Add the payload
        let mut pc = payload.clone();
        packet.append(&mut pc);

        // Generate the payload as it would appear when sent over the wire
        let mut payload_proper = Vec::<u8>::new();
        payload_proper.extend(self.create_packet(VexDeviceCommand::Extended));
        payload_proper.extend(packet.clone());

        // Compute the CRC16
        let crc = crc::Crc::<u16>::new(&VEX_CRC16);
        let mut digest = crc.digest();
        digest.update(&payload_proper);
        let crc = digest.finalize();

        // Pack the crc into the packet
        packet.push(crc.checked_shr(8).unwrap_or(0) as u8);
        packet.push((crc & 0xff) as u8);
        
        

        Ok(packet)
    }
}