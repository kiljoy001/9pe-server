use ninepe_server::protocol;

fn main() {
    println!("Testing message bomb protection...");

    // Test safe constructor
    for size in [1_000_000, 10_000_000, 100_000_000] {
        println!("Testing size: {}", size);
        match protocol::NinePMessage::new_write_safe(u32::MAX, u64::MAX, size) {
            Ok(_) => println!("  Created message of size {}", size),
            Err(e) => println!("  Rejected size {}: {:?}", size, e),
        }
    }

    println!("All tests completed without hanging!");
}