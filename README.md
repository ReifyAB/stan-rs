# stan-rs
[![License Apache 2](https://img.shields.io/badge/License-Apache2-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Crates.io](https://img.shields.io/crates/v/stan.svg)](https://crates.io/crates/stan)

NATS Streaming client wrapper built on top of [NATS.rs](https://github.com/nats-io/nats.rs)

Just a very early prototype.

Only supports publishing for now.

## Installation

```toml
[dependencies]
stan = "0.0.3"
```

## Example useage:

```rust
use std::io;

fn main() -> io::Result<()> {
    let nc = nats::connect("nats://127.0.0.1:4222")?;
    let client = stan::connect(nc, "test-cluster", "rust-client-1")?;

    client.publish("foo", "hello from rust!")
}
```
