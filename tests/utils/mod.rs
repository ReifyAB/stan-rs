use std::{io::BufReader, process::{Child, Command, Stdio}, str::from_utf8, time};
use std::io;
use uuid::Uuid;
use std::io::prelude::*;

pub(crate) struct Server {
    child: Child,
    process_name: String,
    pub port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        Command::new("docker")
            .arg("stop")
            .arg(&self.process_name)
            .output()
            .expect("failed to stop process");
        self.child.wait().unwrap();
    }
}

fn get_free_port() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    port.to_owned()
}

/// Starts a local NATS server that gets killed on drop.
pub(crate) fn server() -> io::Result<Server> {
    let port = get_free_port();
    let process_name = format!("stan-rs-server-test-{}", Uuid::new_v4().to_string());

    let mut child = Command::new("docker")
        .arg("run")
        .args(&["--name", &process_name])
        .args(&["-p", &format!("{}:4222", port)])
        .arg("nats-streaming")
        .stderr(Stdio::piped())
        .spawn()?;

    let stderr = child.stderr.take().unwrap();

    let server = Server {
        process_name,
        child,
        port,
    };

    // wait for stan server
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    loop {
        reader.read_line(&mut line)?;
        if line.contains("Streaming Server is ready") {
            break
        }
    }

    Ok(server)
}

#[allow(dead_code)]
pub(crate) fn next_message(sub: &stan::Subscription) -> io::Result<String> {
    let msg = sub.next_timeout(time::Duration::from_secs(1))?;
    let s = from_utf8(&msg.data).unwrap();
    msg.ack()?;
    Ok(s.to_string())
}
