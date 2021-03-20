use core::time;
use std::{process::{Child, Command}, thread};
use std::{io, net::TcpStream};
use uuid::Uuid;

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
        println!("dropped!");
    }
}

fn get_free_port() -> u16 {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    port.to_owned()
}

fn wait_for_server(port: u16) -> io::Result<()> {
    let mut connect_attempts = 0;
    loop {
        match TcpStream::connect(format!("127.0.0.1:{}", port)) {
            Err(err) if connect_attempts > 100 => {
                return Err(err)
            }
            Err(_) => {
                println!("waiting for server to start...");
                thread::sleep(time::Duration::from_millis(200));
                connect_attempts += 1;
            }
            Ok(_) => {
                println!("server ready!");
                // extra sleep for travis?
                thread::sleep(time::Duration::from_millis(200));
                return Ok(())
            }
        };
    }
}

/// Starts a local NATS server that gets killed on drop.
pub(crate) fn server() -> io::Result<Server> {
    let port = get_free_port();
    let process_name = format!("stan-rs-server-test-{}", Uuid::new_v4().to_string());

    let child = Command::new("docker")
        .arg("run")
        .args(&["--name", &process_name])
        .args(&["-p", &format!("{}:4222", port)])
        .arg("nats-streaming")
        .spawn()?;

    let server = Server {
        process_name,
        child,
        port,
    };

    wait_for_server(port)?;

    Ok(server)
}
