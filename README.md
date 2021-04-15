# SARC library for Rust

[![crates.io](https://img.shields.io/crates/v/sarc-rs)](https://crates.io/crates/sarc-rs)
[![api](https://img.shields.io/badge/api-rustdoc-558b2f)](https://docs.rs/sarc-rs/)
[![license](https://img.shields.io/crates/l/sarc-rs)](https://spdx.org/licenses/MIT.html)

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
