mod binary;
use crate::binary::Binary;

fn main() {
    // Create an instance of the Binary struct
    let binary = Binary::load();

    // Use the binary data and name as needed
    println!("Binary data length: {}", binary.data.len());
    println!("External program: {}", binary.name);
}
