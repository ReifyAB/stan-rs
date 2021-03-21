use std::{io, str::from_utf8};
mod utils;
use logtest::Logger;

#[test]
fn test_without_queue_group() -> io::Result<()> {
    let server = utils::server()?;
    let nats_url = &format!("localhost:{}", server.port);
    let nc = nats::connect(nats_url)?;
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    let mut logger = Logger::start();

    for _ in 0..4 {
        // fork 4 subscriptions without group queue
        sc.subscribe(
            "foo",
            stan::SubscriptionConfig {
                queue_group: None,
                ..Default::default()
            },
        )?.with_handler(|msg| {
            log::info!("received: {}", from_utf8(&msg.data).unwrap());
            Ok(())
        });
    }

    sc.publish("foo", "hello from rust 1")?;

    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");

    Ok(())
}

#[test]
fn test_with_queue_group() -> io::Result<()> {
    let server = utils::server()?;
    let nats_url = &format!("localhost:{}", server.port);
    let nc = nats::connect(nats_url)?;
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    let queue_group = "queue-group-name";
    let mut logger = Logger::start();

    for _ in 0..4 {
        // fork 4 subscriptions on the same queue group
        sc.subscribe(
            "foo",
            stan::SubscriptionConfig {
                queue_group: Some(queue_group),
                ..Default::default()
            },
        )?.with_handler(|msg| {
            log::info!("received: {}", from_utf8(&msg.data).unwrap());
            Ok(())
        });
    }

    sc.publish("foo", "hello from rust 1")?;
    sc.publish("foo", "hello from rust 2")?;
    sc.publish("foo", "hello from rust 3")?;
    sc.publish("foo", "hello from rust 4")?;

    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 2");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 3");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 4");
    Ok(())
}
