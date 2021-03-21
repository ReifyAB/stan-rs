use std::{io, time};
mod utils;
use logtest::Logger;

#[test]
fn test_client_drop() -> io::Result<()> {
    let server = utils::server()?;
    let nats_url = &format!("localhost:{}", server.port);
    let nc = nats::connect(nats_url)?;
    let mut logger = Logger::start();
    {
        let sub = {
            stan::connect(nc.clone(), "test-cluster", "rust-client-1")?
                .subscribe("foo", Default::default())?
        };
        assert_eq!(logger.pop(), None);

        {
            stan::connect(nc.clone(), "test-cluster", "rust-client-2")?
                .publish("foo", "hello from rust 1")?;
        }
        assert_eq!(logger.pop().unwrap().args(), "stan - client closed");

        sub.next_timeout(time::Duration::from_secs(1))?;
    }
    assert_eq!(logger.pop().unwrap().args(), "stan - subscription closed");
    assert_eq!(logger.pop().unwrap().args(), "stan - client closed");

    Ok(())
}
