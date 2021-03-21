use std::{io, thread, time};
mod utils;

fn next_message_starting_at(
    sc: &stan::Client,
    subject: &str,
    start: stan::SubscriptionStart,
) -> io::Result<String> {
    let sub = sc.subscribe(
        subject,
        stan::SubscriptionConfig {
            start,
            ..Default::default()
        },
    )?;
    utils::next_message(&sub)
}

#[test]
fn test_subscription_start() -> io::Result<()> {
    let server = utils::server()?;
    let nats_url = &format!("localhost:{}", server.port);
    let nc = nats::connect(nats_url)?;
    let subject = "foo";
    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    sc.publish(subject, "hello from rust 1")?;
    sc.publish(subject, "hello from rust 2")?;
    thread::sleep(time::Duration::from_millis(50));
    let from_time_start = time::SystemTime::now();
    thread::sleep(time::Duration::from_millis(50));
    sc.publish(subject, "hello from rust 3")?;
    sc.publish(subject, "hello from rust 4")?;
    sc.publish(subject, "hello from rust 5")?;

    assert_eq!(
        "hello from rust 1",
        next_message_starting_at(&sc, subject, stan::SubscriptionStart::AllAvailable)?,
    );

    assert_eq!(
        "hello from rust 3",
        next_message_starting_at(
            &sc,
            subject,
            stan::SubscriptionStart::FromTimestamp(from_time_start)
        )?
    );

    assert_eq!(
        "hello from rust 3",
        next_message_starting_at(
            &sc,
            subject,
            stan::SubscriptionStart::FromPast(time::Duration::from_millis(50))
        )?,
    );

    assert_eq!(
        "hello from rust 4",
        next_message_starting_at(&sc, subject, stan::SubscriptionStart::FromSequence(4))?,
    );

    assert_eq!(
        "hello from rust 5",
        next_message_starting_at(&sc, subject, stan::SubscriptionStart::LastReceived)?
    );

    let new_only_sub = sc.subscribe(
        subject,
        stan::SubscriptionConfig {
            start: stan::SubscriptionStart::NewOnly,
            ..Default::default()
        },
    )?;

    assert_eq!(
        Err(io::ErrorKind::TimedOut),
        utils::next_message(&new_only_sub).map_err(|e| e.kind())
    );

    sc.publish(subject, "hello from rust 6")?;

    assert_eq!("hello from rust 6", utils::next_message(&new_only_sub)?);

    Ok(())
}
