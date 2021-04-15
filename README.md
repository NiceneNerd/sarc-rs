# SARC library for Rust

A simple to use library for parsing and creating Nintendo SARC files in Rust.
Uses zero allocation parsing and *handles file alignment requirements for common
formats and games* like `The Legend of Zelda: Breath of the Wild`.

Sample usage:

```rust
use sarc_rs::{Sarc, SarcWriter};
let data = std::fs::read("test/Dungeon119.pack").unwrap();
let sarc = Sarc::new(&data).unwrap(); // Read a SARC from binary data
for file in sarc.files() { // Iterate files in SARC
    if let Some(name) = file.name {
       println!("File name: {}", name); // Print file name
    }
    println!("File size: {}", file.data.len()); // Print data size
}
```
