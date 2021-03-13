# stan-rs

NATS Streaming client wrapper built on top of [NATS.rs](https://github.com/nats-io/nats.rs)

Just a very early prototype.

Only supports publishing for now.


## Example useage:

```rust
use std::io;

fn main() -> io::Result<()> {
    let nc = nats::connect("nats://127.0.0.1:4222")?;
    let client = stan::connect(nc, "test-cluster", "rust-client-1")?;

    client.publish("foo", "hello from rust!")
}
```
