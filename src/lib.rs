#![doc(html_root_url = "https://docs.rs/stan/0.0.9")]

//! NATS Streaming client wrapper built on top of [NATS.rs](https://github.com/nats-io/nats.rs)
//!
//! Just a very early prototype.
//!
//! Supports publishing and basic subscription.
//!
//! # Examples
//! ```
//! use nats;
//! use std::{io, str::from_utf8};
//!
//! fn main() -> io::Result<()> {
//!     let nc = nats::connect("nats://127.0.0.1:4222")?;
//!     let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
//!
//!     sc.publish("foo", "hello from rust 1")?;
//!
//!     let sub1 = sc
//!         .subscribe("foo", Default::default())?
//!         .with_handler(|msg| {
//!             println!("sub1 got {:?}", from_utf8(&msg.data));
//!             msg.ack()?;
//!             println!("manually acked!");
//!             Ok(())
//!         });
//!
//!     sc.subscribe("foo", Default::default())?
//!         .with_handler(|msg| {
//!             println!("sub 2 got {:?}", from_utf8(&msg.data));
//!             Ok(())
//!         });
//!
//!     sc.publish("foo", "hello from rust 2")?;
//!     sc.publish("foo", "hello from rust 3")?;
//!
//!     sub1.unsubscribe()?;
//!
//!     sc.publish("foo", "hello from rust 4")?;
//!     Ok(())
//! }
//! ```

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

fn new_ack_inbox() -> String {
    DEFAULT_ACKS_SUBJECT.to_owned() + "." + &utils::uuid()
}

fn process_heartbeat(msg: nats::Message) -> io::Result<()> {
    if let Some(reply) = msg.reply {
        msg.client.publish(&reply, None, None, &[])?;
    }
    Ok(())
}

fn process_ack(msg: nats::Message) -> io::Result<()> {
    let ack = proto::PubAck::decode(Bytes::from(msg.data))?;
    if ack.error != "" {
        return Err(io::Error::new(io::ErrorKind::Other, ack.error));
    }
    println!("ack: {}", &ack.guid);
    Ok(())
}

#[derive(Clone)]
pub struct Subscription {
    subscription: nats::Subscription,
    inner: Arc<InnerSub>,
}

struct InnerSub {
    nats_connection: nats::Connection,
    client_id: String,
    subject: String,
    ack_inbox: String,
    durable_name: String,
    unsub_req_subject: String,
}

impl Drop for InnerSub {
    fn drop(&mut self) {
        let req = proto::UnsubscribeRequest {
            client_id: self.client_id.to_owned(),
            subject: self.subject.to_owned(),
            inbox: self.ack_inbox.to_owned(),
            durable_name: self.durable_name.to_owned(),
        };
        let mut buf: Vec<u8> = Vec::new();
        // TODO: better error handling?
        req.encode(&mut buf).unwrap_or_else(|e| println!("{:?}", e));
        self.nats_connection
            .publish(&self.unsub_req_subject, &buf)
            .unwrap_or_else(|e| println!("{:?}", e));
        println!("unsubed")
    }
}

fn ack_msg(
    nats_connection: &nats::Connection,
    ack_inbox: &str,
    subject: &str,
    sequence: &u64,
) -> io::Result<()> {
    let ack = proto::Ack {
        subject: subject.to_string(),
        sequence: sequence.to_owned(),
    };
    let mut buf: Vec<u8> = Vec::new();
    ack.encode(&mut buf)?;
    nats_connection.publish(ack_inbox, &buf)
}

fn nats_msg_to_stan_msg(nats_connection: nats::Connection, ack_inbox: String, msg: nats::Message) -> io::Result<Message> {
    let m = proto::MsgProto::decode(Bytes::from(msg.data))?;
    let subject = m.subject.to_owned();
    let sequence = m.sequence.to_owned();
    let acked: Mutex<bool> = Mutex::new(false);
    let timestamp = time::SystemTime::UNIX_EPOCH
        + time::Duration::from_nanos(m.timestamp.try_into().unwrap());
    let ack = Arc::new(move || {
        let mut a = acked.lock().unwrap();
        if !*a {
            ack_msg(&nats_connection, &ack_inbox, &subject, &sequence)?;
            *a = true;
        }
        Ok(())
    });

    Ok(Message {
        sequence: sequence.to_owned(),
        subject: m.subject.to_owned(),
        data: m.data,
        timestamp,
        redelivered: m.redelivered,
        redelivery_count: m.redelivery_count,
        ack,
    })
}

/// NATS Streaming subscription
impl Subscription {
    /// Process subscription messages in a separate thread.
    /// Messages are automatically acked unless the handler returns an error.
    /// Messages can also be manually acked by calling msg.ack().
    ///
    /// # Examples:
    ///
    /// Automatic ack:
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    /// fn main() -> io::Result<()> {
    ///    let nc = nats::connect("nats://127.0.0.1:4222")?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///
    ///    sc.subscribe("foo", Default::default())?
    ///        .with_handler(|msg| {
    ///            println!("{:?}", from_utf8(&msg.data));
    ///            Ok(())
    ///        });
    ///
    ///    Ok(())
    /// }
    ///```
    ///
    /// Manual ack:
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    /// fn main() -> io::Result<()> {
    ///    let nc = nats::connect("nats://127.0.0.1:4222")?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///
    ///    sc.subscribe("foo", Default::default())?
    ///        .with_handler(|msg| {
    ///            println!("{:?}", from_utf8(&msg.data));
    ///            msg.ack()?;
    ///            println!("this happens after the ack");
    ///            Ok(())
    ///        });
    ///
    ///    Ok(())
    /// }
    ///```
    pub fn with_handler<F>(self, handler: F) -> nats::subscription::Handler
    where
        F: Fn(&Message) -> io::Result<()> + Send + 'static,
    {
        self.subscription.clone().with_handler(move |msg| {
            let nats_connection = self.inner.nats_connection.to_owned();
            let ack_inbox = self.inner.ack_inbox.to_owned();
            let msg = nats_msg_to_stan_msg(nats_connection, ack_inbox, msg)?;

            handler(&msg)?;

            msg.ack()
        })
    }
}

#[derive(Debug)]
pub enum SubscriptionStart {
    /// Only receive new messages, starting from now (Default)
    NewOnly,
    /// Start receiving from the last received message. Use in
    /// combination with a durable queue.
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
        Self::NewOnly
    }
}

#[derive(Debug)]
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
            start: SubscriptionStart::LastReceived,
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
                    Ok(d) => utils::u128_to_i64(d.as_nanos()),
                    Err(err) => Err(io::Error::new(io::ErrorKind::InvalidInput, err)),
                }
            }
            SubscriptionStart::FromPast(d) => utils::u128_to_i64(d.as_nanos()),
            _ => Ok(0),
        }
    }
}

/// Ack function
pub type AckFn = Arc<dyn Fn() -> io::Result<()>>;

#[derive(Clone)]
pub struct Message {
    pub sequence: u64,
    pub subject: String,
    pub data: Vec<u8>,
    pub timestamp: time::SystemTime,
    pub redelivered: bool,
    pub redelivery_count: u32,
    ack: AckFn,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("sequence", &self.sequence)
            .field("subject", &self.subject)
            .field("data", &self.data)
            .field("timestamp", &self.timestamp)
            .field("redelivered", &self.redelivered)
            .field("redeliverd_count", &self.redelivery_count)
            .field("ack", &"Fn() -> io::Result<()>")
            .finish()
    }
}

impl Message {
    /// Ack message
    pub fn ack(&self) -> io::Result<()> {
        (self.ack)()
    }
}

#[derive(Clone)]
pub struct Client {
    nats_connection: nats::Connection,
    cluster_id: String,
    client_id: String,
    conn_id: Vec<u8>,

    pub_prefix: String,
    sub_req_subject: String,
    unsub_req_subject: String,
    close_req_subject: String,
    sub_close_req_subject: String,

    discover_subject: String,
    heartbeat_subject: String,
}

impl Client {
    /// Publish to a given subject. Will return an error if failed to
    /// receive a ack back from the streaming server.
    ///
    /// # Example:
    ///```
    /// use nats;
    /// use std::io;
    /// fn main() -> io::Result<()> {
    ///    let nc = nats::connect("nats://127.0.0.1:4222")?;
    ///    let sc = stan::connect(nc, "test-cluster", "rust-client-1")?;
    ///
    ///    sc.publish("foo", "hello from rust 1")
    /// }
    ///```
    pub fn publish(&self, subject: &str, msg: impl AsRef<[u8]>) -> io::Result<()> {
        let stan_subject = self.pub_prefix.to_owned() + "." + subject;

        let msg = proto::PubMsg {
            client_id: self.client_id.to_owned(),
            guid: utils::uuid(),
            subject: subject.to_owned(),
            reply: "".to_string(), // unused in stan.go
            data: msg.as_ref().to_vec(),
            conn_id: self.conn_id.to_owned(),
            sha256: [].to_vec(), // unused in stan.go
        };

        let mut buf: Vec<u8> = Vec::new();
        msg.encode(&mut buf)?;

        let ack_inbox = new_ack_inbox();
        let ack_sub = self.nats_connection.subscribe(&ack_inbox)?;
        self.nats_connection
            .publish_request(&stan_subject, &ack_inbox, &buf)?;
        let resp = ack_sub.next_timeout(time::Duration::from_secs(1))?;
        process_ack(resp)
    }

    /// Start a subscription.
    ///
    /// # Example:
    ///```
    /// use nats;
    /// use std::{io, str::from_utf8};
    /// fn main() -> io::Result<()> {
    ///    let nc = nats::connect("nats://127.0.0.1:4222")?;
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
        let inbox = self.nats_connection.new_inbox();
        let sub = self.nats_connection.subscribe(&inbox)?;
        let durable_name = config.durable_name.unwrap_or("").to_string();

        let req = proto::SubscriptionRequest {
            client_id: self.client_id.to_owned(),
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
        let res: proto::SubscriptionResponse = self.nats_request(&self.sub_req_subject, req)?;
        if res.error != "" {
            return Err(io::Error::new(io::ErrorKind::Other, res.error));
        }

        Ok(Subscription {
            subscription: sub,
            inner: Arc::new(InnerSub {
                ack_inbox: res.ack_inbox,
                nats_connection: self.nats_connection.to_owned(),
                client_id: self.client_id.to_owned(),
                subject: subject.to_owned(),
                unsub_req_subject: self.unsub_req_subject.to_owned(),
                durable_name: durable_name.to_owned(),
            }),
        })
    }

    fn nats_request<Req: prost::Message, Res: prost::Message + Default>(
        &self,
        subject: &str,
        req: Req,
    ) -> io::Result<Res> {
        let mut buf = Vec::new();
        req.encode(&mut buf)?;
        let resp = self.nats_connection.request(&subject, buf)?;
        Ok(Res::decode(Bytes::from(resp.data))?)
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("cluster_id", &self.cluster_id)
            .field("client_id", &self.client_id)
            .field("conn_id", &self.conn_id)
            .field("pub_prefix", &self.pub_prefix)
            .field("sub_req_subject", &self.sub_req_subject)
            .field("unsub_req_subject", &self.unsub_req_subject)
            .field("close_req_subject", &self.close_req_subject)
            .field("sub_close_req_subject", &self.sub_close_req_subject)
            .field("discover_subject", &self.discover_subject)
            .field("heartbeat_subject", &self.heartbeat_subject)
            .finish()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // TODO: better cleanup?
        // TODO: figure out what to do if we fail here?
        let _res: io::Result<proto::CloseResponse> = self.nats_request(
            &self.close_req_subject,
            proto::CloseRequest {
                client_id: self.client_id.to_owned(),
            },
        );
    }
}

pub fn connect(
    nats_connection: nats::Connection,
    cluster_id: &str,
    client_id: &str,
) -> io::Result<Client> {
    let discover_subject = DEFAULT_DISCOVER_SUBJECT.to_owned() + "." + cluster_id;
    let heartbeat_subject = utils::uuid();
    let _heartbeat_sub = nats_connection
        .subscribe(&heartbeat_subject)?
        .with_handler(process_heartbeat);

    let mut client = Client {
        nats_connection,
        cluster_id: cluster_id.to_owned(),
        client_id: client_id.to_owned(),
        conn_id: utils::uuid().as_bytes().to_owned(),
        discover_subject,
        heartbeat_subject,

        pub_prefix: "".to_string(),
        sub_req_subject: "".to_string(),
        unsub_req_subject: "".to_string(),
        close_req_subject: "".to_string(),
        sub_close_req_subject: "".to_string(),
    };

    let conn_req = proto::ConnectRequest {
        client_id: client_id.to_string(),
        heartbeat_inbox: client.heartbeat_subject.to_owned(),
        protocol: PROTOCOL,
        conn_id: client.conn_id.to_owned(),
        ping_interval: DEFAULT_PING_INTERVAL,
        ping_max_out: DEFAULT_PING_MAX_OUT,
    };

    let conn_resp: proto::ConnectResponse =
        client.nats_request(&client.discover_subject, conn_req)?;
    if conn_resp.error != "" {
        return Err(io::Error::new(io::ErrorKind::Other, conn_resp.error));
    }

    client.pub_prefix = conn_resp.pub_prefix;
    client.sub_req_subject = conn_resp.sub_requests;
    client.unsub_req_subject = conn_resp.unsub_requests;
    client.close_req_subject = conn_resp.close_requests;
    client.sub_close_req_subject = conn_resp.sub_close_requests;

    Ok(client)
}
