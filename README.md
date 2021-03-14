# stan-rs
[![License Apache 2](https://img.shields.io/badge/License-Apache2-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Crates.io](https://img.shields.io/crates/v/stan.svg)](https://crates.io/crates/stan)
[![Documentation](https://docs.rs/nats/badge.svg)](https://docs.rs/nats/)

NATS Streaming client wrapper built on top of [NATS.rs](https://github.com/nats-io/nats.rs)

Just a very early prototype.

Supports publishing and basic subscription.

## Installation

```toml
[dependencies]
nats = "0.9.7"
stan = "0.0.5"
```

## Example useage:

```rust
use nats;
use stan;
use std::{io, str::from_utf8};

fn main() -> io::Result<()> {
    let nc = nats::connect("nats://127.0.0.1:4222")?;
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;

    sc.publish("foo", "hello from rust 1")?;

    let sub = sc
        .subscribe("foo", Some("foo-2"), None)?
        .with_handler(|msg| {
            println!("{:?}", from_utf8(&msg.data));
            Ok(())
        });

    sc.publish("foo", "hello from rust 2")?;
    sc.publish("foo", "hello from rust 3")?;

    sub.unsubscribe()?;

    sc.publish("foo", "hello from rust 4")?;

    Ok(())
}

```

## Development

To start a local nats streaming server for testing:

```
docker run -p 4222:4222 -p 8222:8222 nats-streaming
```
