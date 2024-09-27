use anyhow::Context;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use bytes::BytesMut;
use mqttrs::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = "127.0.0.1:1883";
    let mut mqtt = tcp_connect(host)
        .await
        .context(format!("connecting {host}"))?;
    let mut sess = Session::default();

    send_connect(&mut mqtt).await.context(format!("sending MQTT connect"))?;

    let mut buffer = BytesMut::with_capacity(1024);
    loop {
        let n = mqtt.read_buf(&mut buffer).await.context(format!("reading bytes from {host}"))?;
        if n == 0 {
            println!("<< EOF");
            break;
        }
        let mut bytes = buffer.split();
        match decode_slice(&mut bytes).context(format!("format decoding MQTT packet of {n} bytes"))? {
            None => eprintln!("Not enough data"),
            Some(Packet::Connack(connack)) => {
                println!("<< {connack:?}");
                send_subscribe(&mut mqtt, sess.next_pid(), "mqtt/cat".to_string()).await?;
            },
            Some(Packet::Suback(suback)) => {
                println!("<< {suback:?}");
                send_publish(&mut mqtt, sess.next_pid(), "mqtt/cat", "hello!").await?;
            }
            Some(Packet::Puback(pid)) => {
                let puback = Packet::Puback(pid);
                println!("<< {puback:?}");
            }
            Some(Packet::Publish(publish)) => {
                println!("<< {publish:?}");
                match publish.qospid {
                    QosPid::AtMostOnce => (),
                    QosPid::AtLeastOnce(pid) => {
                        send_puback(&mut mqtt, pid).await?;
                    }
                    QosPid::ExactlyOnce(pid) => {
                        send_puback(&mut mqtt, pid).await?;
                    }
                }
            }
            Some(pkt) => eprintln!("<< {pkt:?}"),
        }
    };

    Ok(())
}

async fn tcp_connect(host: &str) -> anyhow::Result<TcpStream> {
    let addr = host.parse::<SocketAddr>()?;
    let stream = TcpStream::connect(&addr).await?;
    stream.set_nodelay(true)?;
    stream.set_linger(None)?;
    Ok(stream)
}

async fn send_packet<'a>(mqtt: &'a mut TcpStream, pkt: Packet<'a>) -> anyhow::Result<()> {
    let mut buf = [0u8; 1024];
    let len = encode_slice(&pkt, &mut buf)?;
    mqtt.write_all(&mut buf[..len]).await.context("sending bytes")?;
    Ok(())
}

async fn send_connect(mqtt: &mut TcpStream) -> anyhow::Result<()> {
    let connect = Connect {
        protocol: Protocol::MQTT311,
        keep_alive: 60,
        client_id: "mqtt-cat",
        clean_session: true,
        last_will: None,
        username: None,
        password: None
    };
    println!(">> {connect:?}");
    send_packet(mqtt, Packet::Connect(connect)).await
}

async fn send_subscribe(mqtt: &mut TcpStream, pid: Pid, topic_path: String) -> anyhow::Result<()> {
    let subscribe = Subscribe {
        pid,
        topics: vec![SubscribeTopic {
            topic_path,
            qos: QoS::AtMostOnce
        }]
    };
    println!(">> {subscribe:?}");
    send_packet(mqtt, Packet::Subscribe(subscribe)).await
}

async fn send_publish(mqtt: &mut TcpStream, pid: Pid, topic: &str, payload: &str) -> anyhow::Result<()> {
    let publish = Publish {
        dup: false,
        qospid: QosPid::AtLeastOnce(pid),
        retain: false,
        topic_name: topic,
        payload: payload.as_bytes(),
    };
    println!(">> {publish:?}");
    send_packet(mqtt, Packet::Publish(publish)).await
}

async fn send_puback(mqtt: &mut TcpStream, pid: Pid) -> anyhow::Result<()> {
    let puback = Packet::Puback(pid);
    println!(">> {puback:?}");
    send_packet(mqtt, puback).await
}

#[derive(Default)]
struct Session {
    pid: Pid,
}
impl Session {
    pub fn next_pid(&mut self) -> Pid {
        let pid = self.pid;
        self.pid = pid + 1;
        pid
    }
}
