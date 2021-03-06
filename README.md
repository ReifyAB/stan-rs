[![License Apache 2](https://img.shields.io/badge/License-Apache2-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Crates.io](https://img.shields.io/crates/v/stan.svg)](https://crates.io/crates/stan)
[![Documentation](https://docs.rs/stan/badge.svg)](https://docs.rs/stan/)
[![Build Status](https://travis-ci.com/ReifyAB/stan-rs.svg?branch=main)](https://travis-ci.com/ReifyAB/stan-rs)

# stan

NATS Streaming client wrapper built on top of [NATS.rs](https://github.com/nats-io/nats.rs)

## Examples
```rust
use nats;
use std::{io, str::from_utf8, time};

fn main() -> io::Result<()> {
    let nats_url = "nats://127.0.0.1:4222";
    let nc = nats::connect(nats_url)?;
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;

    sc.publish("foo", "hello from rust 1")?;

    sc.subscribe("foo", Default::default())?
        .with_handler(|msg| {
            println!("sub 1 got {:?}", from_utf8(&msg.data));
            Ok(())
        });

    let sub = sc
        .subscribe(
            "foo",
            stan::SubscriptionConfig {
                queue_group: Some("queue-group-name"),
                durable_name: Some("my-durable-queue"),
                start: stan::SubscriptionStart::AllAvailable,
                ..Default::default()
            },
        )?
        .with_handler(|msg| {
            println!("sub 2 got {:?}", from_utf8(&msg.data));
            msg.ack()?;
            println!("manually acked!");
            Ok(())
        });

    for msg in sc.subscribe("foo", Default::default())?.messages() {
        println!("sub 3 got {:?}", from_utf8(&msg.data));
        msg.ack()?;
        break; // just break for the example to run
    }

    for msg in sc
        .subscribe("foo", Default::default())?
        .timeout_iter(time::Duration::from_secs(1))
    {
        println!("sub 4 got {:?}", from_utf8(&msg.data));
        msg.ack()?;
        break; // just break for the example to run
    }

    sc.publish("foo", "hello from rust 2")?;
    sc.publish("foo", "hello from rust 3")?;

    sub.unsubscribe()?;

    sc.publish("foo", "hello from rust 4")?;
    Ok(())
}
```

## Rationale

We were interested in at-least-once delivery with NATS, and the
options here today are NATS Streaming, Lightbridge or Jetstream.

Jetstream is the future of at-least-once delivery on NATS, but is
still in development, while NATS Streaming has been battle tested
in production.

At the same time, the NATS team is providing an awesome rust
client that also has support for Jetstream, but they are not
planning on supporting NATS Streaming (reasonable since Jetstream
is their main focus).

Since NATS Streaming is just a layer on top of NATS, this library
was written to just wrap the nats.rs client to handle the NATS
Streaming protocol, for those like us stuck with NATS Streaming.


## Installation

```toml
[dependencies]
nats = "0.9.7"
stan = "0.0.16"
```

## Development

To start a local nats streaming server:

```sh
docker run -p 4222:4222 -p 8222:8222 nats-streaming
```

Running tests require docker. To run the tests:

```sh
cargo test
```
