use std::io;
mod utils;

#[test]
fn test_without_durable_queue() -> io::Result<()> {
    let server = utils::server()?;
    let nats_url = &format!("localhost:{}", server.port);
    let nc = nats::connect(nats_url)?;
    {
        let sc = stan::connect(nc.clone(), "test-cluster", "rust-client-1")?;
        sc.publish("foo", "hello from rust 1")?;
        let sub = sc.subscribe(
            "foo",
            stan::SubscriptionConfig {
                durable_name: None,
                start: stan::SubscriptionStart::AllAvailable,
                ..Default::default()
            },
        )?;

        assert_eq!(utils::next_message(&sub)?, "hello from rust 1");
    }

    {
        let sc = stan::connect(nc.clone(), "test-cluster", "rust-client-1")?;
        sc.publish("foo", "hello from rust 2")?;
        let sub = sc.subscribe(
            "foo",
            stan::SubscriptionConfig {
                durable_name: None,
                start: stan::SubscriptionStart::AllAvailable,
                ..Default::default()
            },
        )?;

        assert_eq!(utils::next_message(&sub)?, "hello from rust 1");
        assert_eq!(utils::next_message(&sub)?, "hello from rust 2");
    }

    Ok(())
}

#[test]
fn test_with_durable_queue() -> io::Result<()> {
    let server = utils::server()?;
    let nats_url = &format!("localhost:{}", server.port);
    let nc = nats::connect(nats_url)?;
    {
        let sc = stan::connect(nc.clone(), "test-cluster", "rust-client-1")?;
        sc.publish("foo", "hello from rust 1")?;
        let sub = sc.subscribe(
            "foo",
            stan::SubscriptionConfig {
                durable_name: Some("my-durable-queue"),
                start: stan::SubscriptionStart::AllAvailable,
                ..Default::default()
            },
        )?;

        assert_eq!(utils::next_message(&sub)?, "hello from rust 1");
    }

    {
        let sc = stan::connect(nc.clone(), "test-cluster", "rust-client-1")?;
        sc.publish("foo", "hello from rust 2")?;
        let sub = sc.subscribe(
            "foo",
            stan::SubscriptionConfig {
                durable_name: Some("my-durable-queue"),
                start: stan::SubscriptionStart::AllAvailable,
                ..Default::default()
            },
        )?;

        assert_eq!(utils::next_message(&sub)?, "hello from rust 2");
    }

    Ok(())
}
