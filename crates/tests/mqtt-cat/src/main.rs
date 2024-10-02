use crate::actions::Action;
use crate::config::Config;
use crate::config::Message;
use crate::events::Event;
use crate::machines::StateMachine;
use crate::session::Session;
use anyhow::Context;
use bytes::BytesMut;
use clap::Parser;
use mqttrs::*;
use std::net::SocketAddr;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

mod actions;
mod cli;
mod config;
mod events;
mod machines;
mod session;
mod templates;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();

    let config = Config {
        host: args.host().to_string(),
        keep_alive: 60,
        client_id: "mqtt-cat".to_string(),
        clean_session: true,
        subscriptions: vec![SubscribeTopic {
            topic_path: "mqtt-cat/in".to_string(),
            qos: QoS::AtLeastOnce,
        }],
        message_sample: Message {
            topic: "mqtt-cat/out".to_string(),
            payload: "hello!".to_string(),
            qos: QoS::AtMostOnce,
            retain: false,
        },
    };

    match args.command {
        cli::Command::Connect { host } => {
            let sm = StateMachine::sub_client();
            let mqtt = tcp_connect(&host)
                .await
                .context(format!("connecting {}", config.host))?;
            process_events(mqtt, &sm, &config).await
        }

        cli::Command::Bind { host } => {
            let sm = StateMachine::broker();
            let listener = TcpListener::bind(&host)
                .await
                .context(format!("binding {}", config.host))?;
            loop {
                let (mqtt, _) = listener.accept().await?;
                process_events(mqtt, &sm, &config).await?
            }
        }
    }
}

async fn process_events(
    mut mqtt: TcpStream,
    sm: &StateMachine,
    config: &Config,
) -> anyhow::Result<()> {
    let mut session = Session::default();

    let actions = sm.derive_actions(&mut session, config, &Event::TcpConnected);
    react(&mut mqtt, actions)
        .await
        .context("On TCP connect".to_string())?;

    // MQTT Loop
    let mut buffer = BytesMut::with_capacity(1024);
    loop {
        let n = mqtt
            .read_buf(&mut buffer)
            .await
            .context(format!("reading bytes from {}", config.host))?;
        if n == 0 {
            println!("<< EOF");
            break;
        }
        let bytes = buffer.split();
        match decode_slice(&bytes).context(format!("format decoding MQTT packet of {n} bytes"))? {
            None => eprintln!("Not enough data"),
            Some(packet) => {
                println!("<< {packet:?}");
                let event = Event::Received(packet);
                let actions = sm.derive_actions(&mut session, config, &event);
                react(&mut mqtt, actions)
                    .await
                    .context("On MQTT event".to_string())?;
            }
        }
    }

    // Tcp Disconnect
    let actions = sm.derive_actions(&mut session, config, &Event::TcpDisconnected);
    react(&mut mqtt, actions)
        .await
        .context("On TCP disconnect".to_string())?;

    Ok(())
}

async fn tcp_connect(host: &str) -> anyhow::Result<TcpStream> {
    let addr = host.parse::<SocketAddr>()?;
    let stream = TcpStream::connect(&addr).await?;
    stream.set_nodelay(true)?;
    stream.set_linger(None)?;
    Ok(stream)
}

async fn react<'a>(mqtt: &'a mut TcpStream, actions: Vec<Action<'a>>) -> anyhow::Result<()> {
    for action in actions {
        match action {
            Action::Send(packet) => {
                println!(">> {packet:?}");
                send_packet(mqtt, packet).await?
            }
        }
    }
    Ok(())
}

async fn send_packet<'a>(mqtt: &'a mut TcpStream, pkt: Packet<'a>) -> anyhow::Result<()> {
    let mut buf = [0u8; 1024];
    let len = encode_slice(&pkt, &mut buf)?;
    mqtt.write_all(&buf[..len]).await.context("sending bytes")?;
    Ok(())
}
