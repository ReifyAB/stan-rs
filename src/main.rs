use nats;
use stan;
use std::{io, str::from_utf8, thread, time};

fn main() -> io::Result<()> {
    let nc = nats::connect("nats://127.0.0.1:4222")?;
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;

    sc.publish("foo", "hello from rust 1")?;

    let sub1 = sc
        .subscribe("foo", Default::default())?
        .with_handler(|msg| {
            println!("sub1 got {:?}", from_utf8(&msg.data));
            msg.ack()?;
            println!("manually acked!");
            Ok(())
        });

    sc.subscribe("foo", Default::default())?
        .with_handler(|msg| {
            println!("sub 2 got {:?}", from_utf8(&msg.data));
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

    sub1.unsubscribe()?;

    thread::sleep(time::Duration::from_secs(1));
    sc.publish("foo", "hello from rust 4")?;
    Ok(())
}
