extern crate prost_build;
use std::{env, error::Error};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let mut config = prost_build::Config::new();

    config.file_descriptor_set_path(
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR environment variable not set"))
            .join("file_descriptor_set.bin"),
    );

    config.compile_protos(&["src/nats_streaming.proto"], &["src/"])?;
    Ok(())
}
