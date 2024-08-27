use core::sync::atomic::Ordering::Relaxed;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

struct Proxy {
    port: u16,
    stop_tx: tokio::sync::watch::Sender<bool>,
}

impl Proxy {
    async fn start(target_host_port: String, local_port: String) -> Self {
        let bind_addr = format!("127.0.0.1:{local_port}");
        let listener = TcpListener::bind(bind_addr).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(true);
        tokio::spawn(async move {
            let client_count = std::sync::atomic::AtomicU32::new(0);
            loop {
                while *stop_rx.borrow_and_update() {
                    let mut stop = stop_rx.clone();
                    stop.mark_unchanged();
                    if let Ok((mut socket, _)) = listener.accept().await {
                        let client_id = client_count.fetch_add(1, Relaxed);
                        eprintln!("New connection #{client_id}");
                        let mut conn = loop {
                            let Ok(conn) = tokio::net::TcpStream::connect(&target_host_port).await
                            else {
                                eprintln!("Fail to connect {target_host_port}");
                                continue;
                            };
                            eprintln!("Connected #{client_id}");
                            break conn;
                        };
                        tokio::spawn(async move {
                            let (mut read_socket, mut write_socket) = socket.split();
                            let (mut read_conn, mut write_conn) = conn.split();

                            tokio::select! {
                                res = tokio::io::copy(&mut read_socket, &mut write_conn) => { res.unwrap(); },
                                res = tokio::io::copy(&mut read_conn, &mut write_socket) => { res.unwrap(); },
                                _ = stop.changed() => eprintln!("Lost connection #{client_id}"),
                            };

                            write_socket.shutdown().await.unwrap();
                            let _ = write_conn.shutdown().await;
                            eprintln!("Disconnected #{client_id}")
                        });
                    }
                }

                let mut start = stop_rx.clone();
                start.mark_unchanged();
                let _ = start.changed().await;
            }
        });

        Self { port, stop_tx }
    }

    /// Sends a signal to drop all active connections to the proxy
    fn interrupt_connections(&self, up: bool) {
        self.stop_tx.send(up).unwrap()
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        eprintln!("Usage: {} target-host:port [local-port]", args[0]);
        std::process::exit(1);
    }

    let target_host_port = args[1].as_str();
    let bind_port = if args.len() > 2 {
        args[2].as_str()
    } else {
        "0"
    };

    let proxy = Proxy::start(target_host_port.to_string(), bind_port.to_string()).await;
    println!("tunnel {target_host_port} <-> 127.0.0.1:{}", proxy.port);
    println!("=> hit enter to toggle connection up and down");
    println!("");

    let mut input = String::new();
    let mut up = true;
    loop {
        println!("Network is {}", if up { "up" } else { "down" });
        match std::io::stdin().read_line(&mut input) {
            Ok(n) if n > 0 => {
                up = !up;
                proxy.interrupt_connections(up);
            }
            _ => break,
        }
    }
}
