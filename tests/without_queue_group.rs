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
        // fork 4 subscriptions without queue group
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

    sc.subscribe("foo", stan::SubscriptionConfig{
        start: stan::SubscriptionStart::LastReceived,
        ..Default::default()
    })?.next();

    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");
    assert_eq!(logger.pop().unwrap().args(), "received: hello from rust 1");

    Ok(())
}
