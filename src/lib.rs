#![doc(html_root_url = "https://docs.rs/stan/0.0.5")]

use bytes::Bytes;
use prost::Message;
use std::{fmt, io, sync::Arc, time};
use uuid::Uuid;

mod proto;

const DEFAULT_ACKS_SUBJECT: &str = "_STAN.acks";
const DEFAULT_DISCOVER_SUBJECT: &str = "_STAN.discover";
const DEFAULT_ACK_WAIT: i32 = 5;
const DEFAULT_MAX_INFLIGHT: i32 = 1024;
const _DEFAULT_CONNECT_TIMEOUT: i32 = 2;
const _DEFAULT_MAX_PUB_ACKS_INFLIGHT: i32 = 16384;
const PROTOCOL: i32 = 1;
const DEFAULT_PING_INTERVAL: i32 = 5;
const DEFAULT_PING_MAX_OUT: i32 = 88;

fn uuid() -> String {
    Uuid::new_v4().to_string()
}

fn new_ack_inbox() -> String {
    DEFAULT_ACKS_SUBJECT.to_owned() + "." + &uuid()
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

impl Subscription {
    fn ack_msg(&self, m: proto::MsgProto) -> io::Result<()> {
        let ack = proto::Ack {
            subject: m.subject.to_owned(),
            sequence: m.sequence,
        };
        let mut buf: Vec<u8> = Vec::new();
        ack.encode(&mut buf)?;
        self.inner.nats_connection.publish(&self.inner.ack_inbox, &buf)
    }

    pub fn with_handler<F>(self, handler: F) -> nats::subscription::Handler
    where
        F: Fn(proto::MsgProto) -> io::Result<()> + Send + 'static,
    {
        self.subscription.clone().with_handler(move |msg| {
            let m = proto::MsgProto::decode(Bytes::from(msg.data))?;

            handler(m.to_owned())?;

            self.ack_msg(m)
        })
    }
}

impl Client {
    pub fn publish(&self, subject: &str, msg: impl AsRef<[u8]>) -> io::Result<()> {
        let stan_subject = self.pub_prefix.to_owned() + "." + subject;

        let msg = proto::PubMsg {
            client_id: self.client_id.to_owned(),
            guid: uuid(),
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

    pub fn subscribe(&self, subject: &str, queue_group: Option<&str>, durable_name: Option<&str>) -> io::Result<Subscription> {
        let inbox = self.nats_connection.new_inbox();
        let sub = self.nats_connection.subscribe(&inbox)?;
        let durable_name = durable_name.unwrap_or("").to_string();

        let req = proto::SubscriptionRequest {
            client_id: self.client_id.to_owned(),
            subject: subject.to_string(),
            q_group: queue_group.unwrap_or("").to_string(),
            inbox: inbox.to_owned(),
            durable_name: durable_name.to_owned(),
            max_in_flight: DEFAULT_MAX_INFLIGHT,
            ack_wait_in_secs: DEFAULT_ACK_WAIT,
            start_position: proto::StartPosition::LastReceived.into(),
            start_sequence: 0,
            start_time_delta: 0,
        };
        let res: proto::SubscriptionResponse = self.nats_request(&self.sub_req_subject, req)?;
        if res.error != "" {
            return Err(io::Error::new(io::ErrorKind::Other, res.error));
        }

        Ok(Subscription {
            subscription: sub,
            inner: Arc::new(InnerSub{
                ack_inbox: res.ack_inbox,
                nats_connection: self.nats_connection.to_owned(),
                client_id: self.client_id.to_owned(),
                subject: subject.to_owned(),
                unsub_req_subject: self.unsub_req_subject.to_owned(),
                durable_name: durable_name.to_owned(),
            })
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
    let heartbeat_subject = uuid();
    let _heartbeat_sub = nats_connection
        .subscribe(&heartbeat_subject)?
        .with_handler(process_heartbeat);

    let mut client = Client {
        nats_connection,
        cluster_id: cluster_id.to_owned(),
        client_id: client_id.to_owned(),
        conn_id: uuid().as_bytes().to_owned(),
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
