#![doc(html_root_url = "https://docs.rs/stan/0.0.16")]

//! NATS Streaming client wrapper built on top of [NATS.rs](https://github.com/nats-io/nats.rs)
//!
//! # Examples
//! ```
//! use nats;
//! use std::{io, str::from_utf8, time};
//!# mod test_utils;
//!
//! fn main() -> io::Result<()> {
//!     let nats_url = "nats://127.0.0.1:4222";
//!#    let server = test_utils::server()?;
//!#    let nats_url = &format!("localhost:{}", server.port);
//!     let nc = nats::connect(nats_url)?;
//!     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
//!
//!     sc.publish("foo", "hello from rust 1")?;
//!
//!     sc.subscribe("foo", Default::default())?
//!         .with_handler(|msg| {
//!             println!("sub 1 got {:?}", from_utf8(&msg.data));
//!             Ok(())
//!         });
//!
//!     let sub = sc
//!         .subscribe(
//!             "foo",
//!             stan::SubscriptionConfig {
//!                 queue_group: Some("queue-group-name"),
//!                 durable_name: Some("my-durable-queue"),
//!                 start: stan::SubscriptionStart::AllAvailable,
//!                 ..Default::default()
//!             },
//!         )?
//!         .with_handler(|msg| {
//!             println!("sub 2 got {:?}", from_utf8(&msg.data));
//!             msg.ack()?;
//!             println!("manually acked!");
//!             Ok(())
//!         });
//!
//!     for msg in sc.subscribe("foo", Default::default())?.messages() {
//!         println!("sub 3 got {:?}", from_utf8(&msg.data));
//!         msg.ack()?;
//!         break; // just break for the example to run
//!     }
//!
//!     for msg in sc
//!         .subscribe("foo", Default::default())?
//!         .timeout_iter(time::Duration::from_secs(1))
//!     {
//!         println!("sub 4 got {:?}", from_utf8(&msg.data));
//!         msg.ack()?;
//!         break; // just break for the example to run
//!     }
//!
//!     sc.publish("foo", "hello from rust 2")?;
//!     sc.publish("foo", "hello from rust 3")?;
//!
//!     sub.unsubscribe()?;
//!
//!     sc.publish("foo", "hello from rust 4")?;
//!     Ok(())
//! }
//!```
//!
//! # Rationale
//!
//! We were interested in at-least-once delivery with NATS, and the
//! options here today are NATS Streaming, Lightbridge or Jetstream.
//!
//! Jetstream is the future of at-least-once delivery on NATS, but is
//! still in development, while NATS Streaming has been battle tested
//! in production.
//!
//! At the same time, the NATS team is providing an awesome rust
//! client that also has support for Jetstream, but they are not
//! planning on supporting NATS Streaming (reasonable since Jetstream
//! is their main focus).
//!
//! Since NATS Streaming is just a layer on top of NATS, this library
//! was written to just wrap the nats.rs client to handle the NATS
//! Streaming protocol, for those like us stuck with NATS Streaming.
//!

use bytes::Bytes;
use prost::Message as ProstMessage;
use std::{
    convert::TryInto,
    fmt, io,
    sync::{Arc, Mutex},
    time,
};

mod proto;
mod utils;

const DEFAULT_ACKS_SUBJECT: &str = "_STAN.acks";
const DEFAULT_DISCOVER_SUBJECT: &str = "_STAN.discover";
const DEFAULT_ACK_WAIT: i32 = 5;
const DEFAULT_MAX_INFLIGHT: i32 = 1024;
const PROTOCOL: i32 = 1;
const DEFAULT_PING_INTERVAL: i32 = 5;
const DEFAULT_PING_MAX_OUT: i32 = 88;

#[derive(Clone)]
/// NATS Streaming subscription
///
///# Example:
///```
/// use nats;
/// use std::{io, str::from_utf8, time};
///# mod test_utils;
///
/// fn main() -> io::Result<()> {
///     let nats_url = "nats://127.0.0.1:4222";
///#    let server = test_utils::server()?;
///#    let nats_url = &format!("localhost:{}", server.port);
///     let nc = nats::connect(nats_url)?;
///     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
///#
///#     sc.publish("foo", "hello from rust 1")?;
///#
///      let sub1 = sc
///          .subscribe("foo", Default::default())?
///          .with_handler(|msg| {
///              println!("sub1 got {:?}", from_utf8(&msg.data));
///              msg.ack()?;
///              println!("manually acked!");
///              Ok(())
///          });
///
///      sc.subscribe("foo", Default::default())?
///          .with_handler(|msg| {
///              println!("sub 2 got {:?}", from_utf8(&msg.data));
///              Ok(())
///          });
///
///      for msg in sc.subscribe("foo", Default::default())?.messages() {
///          println!("sub 3 got {:?}", from_utf8(&msg.data));
///          msg.ack()?;
///#        break;
///      }
///
///      for msg in sc
///          .subscribe("foo", Default::default())?
///          .timeout_iter(time::Duration::from_secs(1))
///      {
///          println!("sub 4 got {:?}", from_utf8(&msg.data));
///          msg.ack()?;
///#        break;
///      }
///
///      Ok(())
///  }
pub struct Subscription {
    nats_subscription: nats::Subscription,
    inner: Arc<InnerSub>,
    // Keep a reference to Client to only drop it once the
    // subscription is closed. Note that we need it last in the struct
    // to be dropped after we drop the inner subscription
    // (i.e. unsubscribe)
    client: Client,
}

struct InnerSub {
    nats_connection: nats::Connection,
    client_id: String,
    subject: String,
    ack_inbox: String,
    durable_name: String,
    unsub_req_subject: String,
}

impl InnerSub {
    fn unsubscribe(&self) -> io::Result<()> {
        let req = proto::UnsubscribeRequest {
            client_id: self.client_id.to_owned(),
            subject: self.subject.to_owned(),
            inbox: self.ack_inbox.to_owned(),
            durable_name: self.durable_name.to_owned(),
        };
        let mut buf: Vec<u8> = Vec::new();
        req.encode(&mut buf)?;
        self.nats_connection.publish(&self.unsub_req_subject, &buf)
    }
}

impl Drop for InnerSub {
    fn drop(&mut self) {
        // We do not want to unsubscribe a durable subscription by
        // default, only non-durable ones. See:
        // https://github.com/nats-io/stan.go#closing-the-group
        if self.durable_name == "" {
            // TODO: better error handling?
            if let Err(err) = self.unsubscribe() {
                log::error!("stan - error closing subscription: {:?}", err)
            } else {
                log::debug!("stan - subscription closed")
            }
        }
    }
}

impl Subscription {
    /// Returns a blocking message iterator.
    /// Same as calling `iter()`.
    ///```
    ///# use nats;
    ///# use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    ///#  fn main() -> io::Result<()> {
    ///#     let nats_url = "nats://127.0.0.1:4222";
    ///#     let server = test_utils::server()?;
    ///#     let nats_url = &format!("localhost:{}", server.port);
    ///#     let nc = nats::connect(nats_url)?;
    ///#     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///#
    ///#     sc.publish("foo", "hello from rust 1")?;
    ///#
    ///      for msg in sc.subscribe("foo", Default::default())?.messages() {
    ///          println!("received: {:?}", from_utf8(&msg.data));
    ///          msg.ack()?;
    ///#         break; // just break for the example to run
    ///      }
    ///#
    ///#     Ok(())
    ///# }
    ///```
    pub fn messages(&self) -> Iter<'_> {
        Iter { subscription: self }
    }

    /// Returns a blocking message iterator.
    ///```
    ///# use nats;
    ///# use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    ///#  fn main() -> io::Result<()> {
    ///#     let nats_url = "nats://127.0.0.1:4222";
    ///#     let server = test_utils::server()?;
    ///#     let nats_url = &format!("localhost:{}", server.port);
    ///#     let nc = nats::connect(nats_url)?;
    ///#     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///#
    ///#     sc.publish("foo", "hello from rust 1")?;
    ///#
    ///      for msg in sc.subscribe("foo", Default::default())?.iter() {
    ///          println!("received: {:?}", from_utf8(&msg.data));
    ///          msg.ack()?;
    ///#         break; // just break for the example to run
    ///      }
    ///#
    ///#     Ok(())
    ///# }
    ///```
    pub fn iter(&self) -> Iter<'_> {
        Iter { subscription: self }
    }

    /// Returns a non-blocking message iterator.
    ///```
    ///# use nats;
    ///# use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    ///#  fn main() -> io::Result<()> {
    ///#     let nats_url = "nats://127.0.0.1:4222";
    ///#     let server = test_utils::server()?;
    ///#     let nats_url = &format!("localhost:{}", server.port);
    ///#     let nc = nats::connect(nats_url)?;
    ///#     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///#
    ///#     sc.publish("foo", "hello from rust 1")?;
    ///#
    ///      for msg in sc.subscribe("foo", Default::default())?.try_iter() {
    ///          println!("received: {:?}", from_utf8(&msg.data));
    ///          msg.ack()?;
    ///#         break; // just break for the example to run
    ///      }
    ///#
    ///#     Ok(())
    ///# }
    ///```
    pub fn try_iter(&self) -> TryIter<'_> {
        TryIter { subscription: self }
    }

    /// Returns a blocking message iterator with a time
    /// deadline for blocking.
    ///```
    ///# use nats;
    ///# use std::{io, str::from_utf8, time};
    ///# mod test_utils;
    ///#
    ///#  fn main() -> io::Result<()> {
    ///#     let nats_url = "nats://127.0.0.1:4222";
    ///#     let server = test_utils::server()?;
    ///#     let nats_url = &format!("localhost:{}", server.port);
    ///#     let nc = nats::connect(nats_url)?;
    ///#     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///#
    ///#     sc.publish("foo", "hello from rust 1")?;
    ///#
    ///      for msg in sc
    ///          .subscribe("foo", Default::default())?
    ///          .timeout_iter(time::Duration::from_secs(1))
    ///      {
    ///          println!("received: {:?}", from_utf8(&msg.data));
    ///          msg.ack()?;
    ///#         break; // just break for the example to run
    ///      }
    ///#     Ok(())
    ///# }
    ///```
    pub fn timeout_iter(&self, timeout: time::Duration) -> TimeoutIter<'_> {
        TimeoutIter {
            subscription: self,
            to: timeout,
        }
    }

    /// Process subscription messages in a separate thread.
    /// Messages are automatically acked unless the handler returns an error.
    /// Messages can also be manually acked by calling msg.ack().
    ///
    /// # Examples:
    ///
    /// Automatic ack:
    ///```
    ///# use nats;
    ///# use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    ///# fn main() -> io::Result<()> {
    ///#    let nats_url = "nats://127.0.0.1:4222";
    ///#    let server = test_utils::server()?;
    ///#    let nats_url = &format!("localhost:{}", server.port);
    ///#    let nc = nats::connect(nats_url)?;
    ///#    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///#
    ///     sc.subscribe("foo", Default::default())?
    ///         .with_handler(|msg| {
    ///             println!("{:?}", from_utf8(&msg.data));
    ///             Ok(())
    ///         });
    ///
    ///#    Ok(())
    ///# }
    ///```
    ///
    /// Manual ack:
    ///```
    ///# use nats;
    ///# use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    ///# fn main() -> io::Result<()> {
    ///#    let nats_url = "nats://127.0.0.1:4222";
    ///#    let server = test_utils::server()?;
    ///#    let nats_url = &format!("localhost:{}", server.port);
    ///#    let nc = nats::connect(nats_url)?;
    ///#    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///#
    ///     sc.subscribe("foo", Default::default())?
    ///         .with_handler(|msg| {
    ///             println!("{:?}", from_utf8(&msg.data));
    ///             msg.ack()?;
    ///             println!("this happens after the ack");
    ///             Ok(())
    ///         });
    ///
    ///#    sc.publish("foo", "hello from rust 1")?;
    ///#    Ok(())
    ///# }
    ///```
    pub fn with_handler<F>(self, handler: F) -> nats::subscription::Handler
    where
        F: Fn(&Message) -> io::Result<()> + Send + 'static,
    {
        self.nats_subscription.clone().with_handler(move |msg| {
            let nats_connection = self.inner.nats_connection.to_owned();
            let ack_inbox = self.inner.ack_inbox.to_owned();
            let msg = Message::from_nats_message(msg, nats_connection, ack_inbox)?;
            handler(&msg)?;
            msg.ack()
        })
    }

    /// Get the next message with blocking, or None if the subscription has been closed
    /// Note: the message needs to be manually acked!
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    /// fn main() -> io::Result<()> {
    ///    let nats_url = "nats://127.0.0.1:4222";
    ///#   let server = test_utils::server()?;
    ///#   let nats_url = &format!("localhost:{}", server.port);
    ///    let nc = nats::connect(nats_url)?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///    sc.publish("foo", "hello from rust 1")?;
    ///
    ///    let sub = sc.subscribe("foo", Default::default())?;
    ///    if let Some(msg) = sub.next() {
    ///       println!("received: {:?}", from_utf8(&msg.data));
    ///       msg.ack()?
    ///    }
    ///
    ///    Ok(())
    /// }
    ///```
    pub fn next(&self) -> Option<Message> {
        let msg = self.nats_subscription.next()?;
        let nats_connection = self.inner.nats_connection.to_owned();
        let ack_inbox = self.inner.ack_inbox.to_owned();
        Message::from_nats_message(msg, nats_connection, ack_inbox).ok()
    }

    /// Get the next message without blocking, or None if none available
    /// Note: the message needs to be manually acked!
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    /// fn main() -> io::Result<()> {
    ///    let nats_url = "nats://127.0.0.1:4222";
    ///#   let server = test_utils::server()?;
    ///#   let nats_url = &format!("localhost:{}", server.port);
    ///    let nc = nats::connect(nats_url)?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///    sc.publish("foo", "hello from rust 1")?;
    ///
    ///    let sub = sc.subscribe("foo", Default::default())?;
    ///    if let Some(msg) = sub.try_next() {
    ///       println!("received: {:?}", from_utf8(&msg.data));
    ///       msg.ack()?
    ///    }
    ///
    ///    Ok(())
    /// }
    ///```
    pub fn try_next(&self) -> Option<Message> {
        let msg = self.nats_subscription.try_next()?;
        let nats_connection = self.inner.nats_connection.to_owned();
        let ack_inbox = self.inner.ack_inbox.to_owned();
        Message::from_nats_message(msg, nats_connection, ack_inbox).ok()
    }

    /// Get the next message without blocking, or None if none available
    /// Note: the message needs to be manually acked!
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    /// fn main() -> io::Result<()> {
    ///    let nats_url = "nats://127.0.0.1:4222";
    ///#   let server = test_utils::server()?;
    ///#   let nats_url = &format!("localhost:{}", server.port);
    ///    let nc = nats::connect(nats_url)?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///    sc.publish("foo", "hello from rust 1")?;
    ///
    ///    let sub = sc.subscribe("foo", Default::default())?;
    ///    if let Ok(msg) = sub.next_timeout(std::time::Duration::from_secs(1)) {
    ///       println!("received: {:?}", from_utf8(&msg.data));
    ///       msg.ack()?
    ///    }
    ///
    ///    Ok(())
    /// }
    ///```
    pub fn next_timeout(&self, timeout: time::Duration) -> io::Result<Message> {
        let msg = self.nats_subscription.next_timeout(timeout)?;
        let nats_connection = self.inner.nats_connection.to_owned();
        let ack_inbox = self.inner.ack_inbox.to_owned();
        Message::from_nats_message(msg, nats_connection, ack_inbox)
    }

    /// Close this subscription.
    ///
    /// For subscriptions that are not durable (i.e. with no
    /// durable_name), this is called automatically when the
    /// subscription is dropped.
    ///
    /// For durable subscriptions, beware that unsubscribing all the
    /// clients will also delete the durable queue.
    ///
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    /// fn main() -> io::Result<()> {
    ///    let nats_url = "nats://127.0.0.1:4222";
    ///#   let server = test_utils::server()?;
    ///#   let nats_url = &format!("localhost:{}", server.port);
    ///    let nc = nats::connect(nats_url)?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///
    ///    let sub = sc.subscribe("foo", Default::default())?;
    ///    sub.unsubscribe();
    ///
    ///    Ok(())
    /// }
    ///```
    pub fn unsubscribe(&self) -> io::Result<()> {
        self.inner.unsubscribe()
    }
}

/// A non-blocking iterator over messages from a `Subscription`
pub struct TryIter<'a> {
    subscription: &'a Subscription,
}

impl<'a> Iterator for TryIter<'a> {
    type Item = Message;
    fn next(&mut self) -> Option<Self::Item> {
        self.subscription.try_next()
    }
}

/// An iterator over messages from a `Subscription`
pub struct Iter<'a> {
    subscription: &'a Subscription,
}

impl<'a> Iterator for Iter<'a> {
    type Item = Message;
    fn next(&mut self) -> Option<Self::Item> {
        self.subscription.next()
    }
}

/// An iterator over messages from a `Subscription`
pub struct IntoIter {
    subscription: Subscription,
}

impl Iterator for IntoIter {
    type Item = Message;
    fn next(&mut self) -> Option<Self::Item> {
        self.subscription.next()
    }
}

/// An iterator over messages from a `Subscription`
/// where `None` will be returned if a new `Message`
/// has not been received by the end of a timeout.
pub struct TimeoutIter<'a> {
    subscription: &'a Subscription,
    to: time::Duration,
}

impl<'a> Iterator for TimeoutIter<'a> {
    type Item = Message;
    fn next(&mut self) -> Option<Self::Item> {
        self.subscription.next_timeout(self.to).ok()
    }
}

#[derive(Debug)]
/// List of possible starting positions for a new `Subscription`
pub enum SubscriptionStart {
    /// Only receive new messages, starting from now
    NewOnly,
    /// Start receiving from the last received message (default)
    LastReceived,
    /// Send all available messages on the subject
    AllAvailable,
    /// Start at a given message sequence. You can use that to build your own durable queue
    FromSequence(u64),
    /// Replay starting from a given timestamp (need to be in the past)
    FromTimestamp(time::SystemTime),
    /// Replay from a duration in the past
    FromPast(time::Duration),
}

impl Default for SubscriptionStart {
    fn default() -> Self {
        Self::LastReceived
    }
}

#[derive(Debug)]
/// Configuration to pass to subscription. Defaults to no queue group,
/// no durable queue and starts from last received message.
pub struct SubscriptionConfig<'a> {
    /// Name of the queue group, see: https://docs.nats.io/nats-concepts/queue
    pub queue_group: Option<&'a str>,
    /// Set this to keep position after reconnect, see: https://docs.nats.io/developing-with-nats-streaming/durables
    pub durable_name: Option<&'a str>,
    /// Position to start the subscription from
    pub start: SubscriptionStart,
    pub max_in_flight: i32,
    pub ack_wait_in_secs: i32,
}

impl<'a> Default for SubscriptionConfig<'a> {
    fn default() -> Self {
        Self {
            queue_group: None,
            durable_name: None,
            start: SubscriptionStart::default(),
            max_in_flight: DEFAULT_MAX_INFLIGHT,
            ack_wait_in_secs: DEFAULT_ACK_WAIT,
        }
    }
}

impl<'a> SubscriptionConfig<'a> {
    fn start_position(&self) -> i32 {
        match self.start {
            SubscriptionStart::NewOnly => proto::StartPosition::NewOnly,
            SubscriptionStart::LastReceived => proto::StartPosition::LastReceived,
            SubscriptionStart::AllAvailable => proto::StartPosition::First,
            SubscriptionStart::FromSequence(_) => proto::StartPosition::SequenceStart,
            SubscriptionStart::FromTimestamp(_) | SubscriptionStart::FromPast(_) => {
                proto::StartPosition::TimeDeltaStart
            }
        }
        .into()
    }

    fn start_sequence(&self) -> u64 {
        if let SubscriptionStart::FromSequence(seq) = self.start {
            seq
        } else {
            0
        }
    }

    fn start_time_delta(&self) -> io::Result<i64> {
        match self.start {
            SubscriptionStart::FromTimestamp(t) => {
                let now = time::SystemTime::now();
                if t > now {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "SubscriptionStart::FromTimestamp is in the future",
                    ));
                }
                match now.duration_since(t) {
                    Ok(d) => Ok(-utils::u128_to_i64(d.as_nanos())?),
                    Err(err) => Err(io::Error::new(io::ErrorKind::InvalidInput, err)),
                }
            }
            SubscriptionStart::FromPast(d) => Ok(-utils::u128_to_i64(d.as_nanos())?),
            _ => Ok(0),
        }
    }
}

#[derive(Debug, Clone)]
/// NATS Streaming message received on a given `Subscription`
pub struct Message {
    pub sequence: u64,
    pub subject: String,
    pub data: Vec<u8>,
    pub timestamp: time::SystemTime,
    pub redelivered: bool,
    pub redelivery_count: u32,
    nats_connection: nats::Connection,
    ack_inbox: String,
    acked: Arc<Mutex<bool>>,
}

impl Message {
    fn from_nats_message(
        msg: nats::Message,
        nats_connection: nats::Connection,
        ack_inbox: String,
    ) -> io::Result<Self> {
        let m = proto::MsgProto::decode(Bytes::from(msg.data))?;
        let sequence = m.sequence.to_owned();
        let timestamp = time::SystemTime::UNIX_EPOCH
            + time::Duration::from_nanos(m.timestamp.try_into().unwrap());
        let acked = Arc::new(Mutex::new(false));

        Ok(Self {
            sequence: sequence.to_owned(),
            subject: m.subject.to_owned(),
            data: m.data,
            timestamp,
            redelivered: m.redelivered,
            redelivery_count: m.redelivery_count,
            nats_connection,
            ack_inbox,
            acked,
        })
    }

    /// Ack message
    pub fn ack(&self) -> io::Result<()> {
        let mut acked = self.acked.lock().unwrap();
        if !*acked {
            let ack = proto::Ack {
                subject: self.subject.to_string(),
                sequence: self.sequence.to_owned(),
            };
            let mut buf: Vec<u8> = Vec::new();
            ack.encode(&mut buf)?;
            self.nats_connection.publish(&self.ack_inbox, &buf)?;
            *acked = true;
        }
        Ok(())
    }
}

#[derive(Clone)]
/// NATS Streaming client
pub struct Client {
    cluster_id: String,
    conn_id: Vec<u8>,
    discover_subject: String,
    heartbeat_subject: String,
    inner: Arc<InnerClient>,
}

struct InnerClient {
    nats_connection: nats::Connection,
    client_id: String,
    pub_prefix: String,
    sub_req_subject: String,
    unsub_req_subject: String,
    close_req_subject: String,
    sub_close_req_subject: String,
}

fn nats_request<Req: prost::Message, Res: prost::Message + Default>(
    nats_connection: nats::Connection,
    subject: &str,
    req: Req,
) -> io::Result<Res> {
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    let resp = nats_connection.request_timeout(&subject, buf, time::Duration::from_secs(2))?;
    Ok(Res::decode(Bytes::from(resp.data))?)
}

impl Drop for InnerClient {
    fn drop(&mut self) {
        let res: io::Result<proto::CloseResponse> = nats_request(
            self.nats_connection.clone(),
            &self.close_req_subject,
            proto::CloseRequest {
                client_id: self.client_id.to_owned(),
            },
        );

        if let Err(err) = res {
            log::error!("stan - error closing client: {}", err)
        } else {
            log::debug!("stan - client closed")
        }
    }
}

impl Client {
    /// Start a new client, establishing a new NATS Streaming connection,
    /// Same as stan::connect().
    ///
    /// # Example:
    ///```
    ///# use nats;
    ///# use std::io;
    ///# mod test_utils;
    ///#
    ///# fn main() -> io::Result<()> {
    ///    let nats_url = "nats://127.0.0.1:4222";
    ///#   let server = test_utils::server()?;
    ///#   let nats_url = &format!("localhost:{}", server.port);
    ///    let nc = nats::connect(nats_url)?;
    ///    let sc = stan::Client::start(nc, "test-cluster", "rust-client-1")?;
    ///#    Ok(())
    ///# }
    ///```
    pub fn start(
        nats_connection: nats::Connection,
        cluster_id: &str,
        client_id: &str,
    ) -> io::Result<Client> {
        let discover_subject = DEFAULT_DISCOVER_SUBJECT.to_owned() + "." + cluster_id;
        let heartbeat_subject = utils::uuid();
        let conn_id = utils::uuid().as_bytes().to_owned();

        // Start heartbeat handler
        nats_connection
            .subscribe(&heartbeat_subject)?
            .with_handler(|msg| {
                if let Some(reply) = msg.reply {
                    msg.client.publish(&reply, None, None, &[])?;
                }
                Ok(())
            });

        let conn_req = proto::ConnectRequest {
            client_id: client_id.to_string(),
            heartbeat_inbox: heartbeat_subject.to_owned(),
            protocol: PROTOCOL,
            conn_id: conn_id.to_owned(),
            ping_interval: DEFAULT_PING_INTERVAL,
            ping_max_out: DEFAULT_PING_MAX_OUT,
        };

        let conn_resp: proto::ConnectResponse =
            nats_request(nats_connection.clone(), &discover_subject, conn_req)?;
        if conn_resp.error != "" {
            return Err(io::Error::new(io::ErrorKind::Other, conn_resp.error));
        }

        Ok(Client {
            cluster_id: cluster_id.to_owned(),
            conn_id: conn_id.to_owned(),
            discover_subject,
            heartbeat_subject,

            inner: Arc::new(InnerClient {
                nats_connection,
                client_id: client_id.to_owned(),
                pub_prefix: conn_resp.pub_prefix,
                sub_req_subject: conn_resp.sub_requests,
                unsub_req_subject: conn_resp.unsub_requests,
                close_req_subject: conn_resp.close_requests,
                sub_close_req_subject: conn_resp.sub_close_requests,
            }),
        })
    }

    /// Publish to a given subject. Will return an error if failed to
    /// receive a ack back from the streaming server.
    ///
    /// # Example:
    ///```
    ///# use nats;
    ///# use std::io;
    ///# mod test_utils;
    ///#
    /// fn main() -> io::Result<()> {
    ///    let nats_url = "nats://127.0.0.1:4222";
    ///#   let server = test_utils::server()?;
    ///#   let nats_url = &format!("localhost:{}", server.port);
    ///    let nc = nats::connect(nats_url)?;
    ///#    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///#
    ///    sc.publish("foo", "hello from rust 1")
    ///# }
    ///```
    pub fn publish(&self, subject: &str, msg: impl AsRef<[u8]>) -> io::Result<()> {
        let stan_subject = self.inner.pub_prefix.to_owned() + "." + subject;

        let msg = proto::PubMsg {
            client_id: self.inner.client_id.to_owned(),
            guid: utils::uuid(),
            subject: subject.to_owned(),
            reply: "".to_string(), // unused in stan.go
            data: msg.as_ref().to_vec(),
            conn_id: self.conn_id.to_owned(),
            sha256: [].to_vec(), // unused in stan.go
        };

        let ack_inbox = DEFAULT_ACKS_SUBJECT.to_owned() + "." + &utils::uuid();
        let ack_sub = self.inner.nats_connection.subscribe(&ack_inbox)?;

        let mut buf: Vec<u8> = Vec::new();
        msg.encode(&mut buf)?;
        self.inner
            .nats_connection
            .publish_request(&stan_subject, &ack_inbox, &buf)?;

        let resp = ack_sub.next_timeout(time::Duration::from_secs(1))?;
        let ack = proto::PubAck::decode(Bytes::from(resp.data))?;
        if ack.error != "" {
            return Err(io::Error::new(io::ErrorKind::Other, ack.error));
        }
        Ok(())
    }

    /// Start a subscription.
    ///
    /// # Example:
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    ///# mod test_utils;
    ///#
    /// fn main() -> io::Result<()> {
    ///    let nats_url = "nats://127.0.0.1:4222";
    ///#   let server = test_utils::server()?;
    ///#   let nats_url = &format!("localhost:{}", server.port);
    ///    let nc = nats::connect(nats_url)?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///
    ///    sc.publish("foo", "hello from rust 1")?;
    ///
    ///    let sub = sc
    ///        .subscribe("foo", Default::default())?
    ///        .with_handler(|msg| {
    ///            println!("{:?}", from_utf8(&msg.data));
    ///            Ok(())
    ///        });
    ///
    ///    sc.publish("foo", "hello from rust 2")?;
    ///    sc.publish("foo", "hello from rust 3")
    /// }
    ///```
    pub fn subscribe(&self, subject: &str, config: SubscriptionConfig) -> io::Result<Subscription> {
        let inbox = self.inner.nats_connection.new_inbox();
        let sub = self.inner.nats_connection.subscribe(&inbox)?;
        let durable_name = config.durable_name.unwrap_or("").to_string();

        let req = proto::SubscriptionRequest {
            client_id: self.inner.client_id.to_owned(),
            subject: subject.to_string(),
            q_group: config.queue_group.unwrap_or("").to_string(),
            inbox: inbox.to_owned(),
            durable_name: durable_name.to_owned(),
            max_in_flight: config.max_in_flight,
            ack_wait_in_secs: config.ack_wait_in_secs,
            start_position: config.start_position(),
            start_sequence: config.start_sequence(),
            start_time_delta: config.start_time_delta()?,
        };
        let res: proto::SubscriptionResponse = nats_request(
            self.inner.nats_connection.clone(),
            &self.inner.sub_req_subject,
            req,
        )?;
        if res.error != "" {
            return Err(io::Error::new(io::ErrorKind::Other, res.error));
        }

        Ok(Subscription {
            nats_subscription: sub,
            client: self.clone(),
            inner: Arc::new(InnerSub {
                ack_inbox: res.ack_inbox,
                nats_connection: self.inner.nats_connection.to_owned(),
                client_id: self.inner.client_id.to_owned(),
                subject: subject.to_owned(),
                unsub_req_subject: self.inner.unsub_req_subject.to_owned(),
                durable_name: durable_name.to_owned(),
            }),
        })
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("cluster_id", &self.cluster_id)
            .field("client_id", &self.inner.client_id)
            .field("conn_id", &self.conn_id)
            .field("pub_prefix", &self.inner.pub_prefix)
            .field("sub_req_subject", &self.inner.sub_req_subject)
            .field("unsub_req_subject", &self.inner.unsub_req_subject)
            .field("close_req_subject", &self.inner.close_req_subject)
            .field("sub_close_req_subject", &self.inner.sub_close_req_subject)
            .field("discover_subject", &self.discover_subject)
            .field("heartbeat_subject", &self.heartbeat_subject)
            .finish()
    }
}

/// Establishes a new NATS Streaming connection, returning a `Client`
///
/// # Example:
///```
///# use nats;
///# use std::io;
///# mod test_utils;
///#
///# fn main() -> io::Result<()> {
///    let nats_url = "nats://127.0.0.1:4222";
///#   let server = test_utils::server()?;
///#   let nats_url = &format!("localhost:{}", server.port);
///    let nc = nats::connect(nats_url)?;
///     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
///#    Ok(())
///# }
///```
pub fn connect(
    nats_connection: nats::Connection,
    cluster_id: &str,
    client_id: &str,
) -> io::Result<Client> {
    Client::start(nats_connection, cluster_id, client_id)
}
