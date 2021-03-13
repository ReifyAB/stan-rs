use std::{io, time};
use bytes::Bytes;
use uuid::Uuid;
use prost::Message;

mod proto;

const DEFAULT_ACKS_SUBJECT: &str = "_STAN.acks";
const DEFAULT_DISCOVER_SUBJECT: &str = "_STAN.discover";
const DEFAULT_ACK_WAIT: u32 = 5;
const DEFAULT_MAX_INFLIGHT: i32 = 1024;
const DEFAULT_CONNECT_TIMEOUT: i32 = 2;
const DEFAULT_MAX_PUB_ACKS_INFLIGHT: i32 = 16384;
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
    println!("Received heartbeat {}", &msg);
    if let Some(reply) = msg.reply {
        println!("respond to heartbeat");
        msg.client.publish(&reply, None, None, &[])?;
    }
    Ok(())
}

fn process_ack(msg: nats::Message) -> io::Result<()> {
    let ack = proto::PubAck::decode(Bytes::from(msg.data))?;
    if ack.error != "" {
        return Err(io::Error::new(io::ErrorKind::Other, ack.error))
    }
    println!("ack: {}", &ack.guid);
    Ok(())
}


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
    heartbeat_sub: nats::subscription::Handler,
}

impl Client {
    pub fn publish(&self, subject: &str, msg: impl AsRef<[u8]>) -> io::Result<()> {
        let stan_subject = self.pub_prefix.to_owned() + "." + subject;

        let msg = proto::PubMsg{
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
        self.nats_connection.publish_request(&stan_subject, &ack_inbox, &buf)?;
        let resp = ack_sub.next_timeout(time::Duration::from_secs(DEFAULT_ACK_WAIT.into()))?;
        process_ack(resp)
    }

    fn nats_request<Req: Message, Res: Message + Default>(&self, req: Req) -> io::Result<Res>{
        let mut buf = Vec::new();
        req.encode(&mut buf).unwrap();
        let resp = self.nats_connection.request(&self.discover_subject, buf)?;
        Ok(Res::decode(Bytes::from(resp.data))?)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let res: io::Result<proto::CloseResponse> = self.nats_request(proto::CloseRequest{
            client_id: self.client_id.to_owned(),
        });
    }
}

pub fn connect(nats_connection: nats::Connection, cluster_id: &str, client_id: &str) -> io::Result<Client> {
    let discover_subject = DEFAULT_DISCOVER_SUBJECT.to_owned() + "." + cluster_id;
    let heartbeat_subject = uuid();
    let heartbeat_sub = nats_connection.subscribe(&heartbeat_subject)?.with_handler(process_heartbeat);
    let conn_id = uuid().as_bytes().to_owned();

    let mut client = Client{
        nats_connection,
        cluster_id: cluster_id.to_owned(),
        client_id: client_id.to_owned(),
        conn_id,
        discover_subject,
        heartbeat_subject,
        heartbeat_sub,

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

    let conn_resp: proto::ConnectResponse = client.nats_request(conn_req)?;
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
