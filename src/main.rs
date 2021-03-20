use nats;
use stan;
use std::{io, str::from_utf8, thread, time};
mod test_utils;

fn main() -> io::Result<()> {
    let server = test_utils::server()?;
    let nats_url = &format!("localhost:{}", server.port);

    let nc = nats::connect(nats_url)?;
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;

    sc.publish("foo", "hello from rust 1")?;

    let sub1 = sc
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


    let sc2 = sc.clone();
    thread::Builder::new()
        .name("stan_loop_1".to_string())
        .spawn(move || {
            for msg in sc2.subscribe("foo", Default::default()).unwrap().messages() {
                println!("sub 3 got {:?}", from_utf8(&msg.data));
                msg.ack().unwrap();
                break; // just break for the example to run
            }
        })?;

    let sc3 = sc.clone();
    thread::Builder::new()
        .name("stan_loop_1".to_string())
        .spawn(move || {
            for msg in sc3
                .subscribe("foo", Default::default()).unwrap()
                .timeout_iter(time::Duration::from_secs(1))
            {
                println!("sub 4 got {:?}", from_utf8(&msg.data));
                msg.ack().unwrap();
                break; // just break for the example to run
            }
        })?;

    sc.publish("foo", "hello from rust 2")?;
    sc.publish("foo", "hello from rust 3")?;

    sub1.unsubscribe()?;

    thread::sleep(time::Duration::from_secs(1));
    sc.publish("foo", "hello from rust 4")?;
    Ok(())
}
