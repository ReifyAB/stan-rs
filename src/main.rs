use nats;
use stan;
use std::{io, str::from_utf8, thread, time};

fn main() -> io::Result<()> {
    let nc = nats::connect("nats://127.0.0.1:4222")?;
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;

    sc.publish("foo", "hello from rust 1")?;

    let sub = sc
        .subscribe("foo", Default::default())?
        .with_handler(|msg| {
            println!("{:?}", from_utf8(msg.data));
            Ok(())
        });

    sc.publish("foo", "hello from rust 2")?;
    sc.publish("foo", "hello from rust 3")?;

    sub.unsubscribe()?;

    thread::sleep(time::Duration::from_secs(1));
    sc.publish("foo", "hello from rust 4")?;
    Ok(())
}
