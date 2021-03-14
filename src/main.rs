use nats;
use stan;
use std::{io, thread, time};

fn main() -> io::Result<()> {
    let nc = nats::connect("nats://127.0.0.1:4222")?;
    let client = stan::connect(nc, "test-cluster", "rust-client-1")?;

    println!("Connected: {:?}", client);

    println!("sending message 1");
    client.publish("foo", "hello from rust 1")?;

    client.subscribe("foo", Some("foo"))?.with_handler(|msg| {
        println!("{:?}", msg);
        Ok(())
    });

    client.publish("foo", "hello from rust 2")?;
    println!("sending message 2");
    client.publish("foo", "hello from rust 3")?;

    thread::sleep(time::Duration::from_secs(100));
    client.publish("foo", "hello from rust 4")?;
    Ok(())
}
