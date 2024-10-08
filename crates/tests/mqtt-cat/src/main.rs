use crate::actions::Action;
use crate::config::Config;
use crate::events::Event;
use crate::events::ExpectedEvent;
use crate::machines::StateMachine;
use crate::messages::Message;
use crate::session::Session;
use crate::timer::TimerSender;
use anyhow::Context;
use bytes::BytesMut;
use clap::Parser;
use mqttrs::*;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::select;
use tokio::sync::mpsc;

mod actions;
mod cli;
mod config;
mod events;
mod machines;
mod messages;
mod session;
mod templates;
mod timer;

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
            qos: QoS::ExactlyOnce,
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
            let messages = message_generator(
                Message {
                    topic: "mqtt-cat/out".to_string(),
                    payload: "hello!".to_string(),
                    qos: QoS::AtLeastOnce,
                    retain: false,
                },
                Duration::from_millis(2500)
            );
            let mqtt = tcp_connect(&host)
                .await
                .context(format!("connecting {}", config.host))?;
            process_events(mqtt, &sm, &config, messages).await
        }

        cli::Command::Bind { host } => {
            let sm = StateMachine::broker();
            let listener = TcpListener::bind(&host)
                .await
                .context(format!("binding {}", config.host))?;
            loop {
                let (mqtt, _) = listener.accept().await?;
                let messages = message_generator(
                    Message {
                        topic: "mqtt-cat/in".to_string(),
                        payload: "hello subscriber!".to_string(),
                        qos: QoS::ExactlyOnce,
                        retain: false,
                    },
                    Duration::from_millis(2500)
                );
                process_events(mqtt, &sm, &config, messages).await?
            }
        }
    }
}

async fn process_events(
    mut mqtt: TcpStream,
    sm: &StateMachine,
    config: &Config,
    mut messages: mpsc::Receiver<Message>,
) -> anyhow::Result<()> {
    let mut session = Session::default();
    let (mut timer_sender, mut timer_listener) = timer::channel(Duration::from_millis(250));

    let actions = sm.derive_actions(&mut session, config, &Event::TcpConnected);
    react(&mut mqtt, &mut timer_sender, actions)
        .await
        .context("On TCP connect".to_string())?;

    // MQTT Loop
    let mut buffer = BytesMut::with_capacity(1024);
    loop {
        select! {
            read = mqtt.read_buf(&mut buffer) => {
                let n = read.context(format!("reading bytes from {}", config.host))?;
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
                        react(&mut mqtt, &mut timer_sender, actions)
                            .await
                            .context("On MQTT event".to_string())?;
                    }
                }
            }

            Some(message) = messages.recv() => {
                let event = Event::MessageQueued(message);
                let actions = sm.derive_actions(&mut session, config, &event);
                react(&mut mqtt, &mut timer_sender, actions)
                    .await
                    .context("On new message".to_string())?;
            }

            Some((pid,expected)) = timer_listener.timeout() => {
                let event = Event::Timeout { pid, expected };
                let actions = sm.derive_actions(&mut session, config, &event);
                react(&mut mqtt, &mut timer_sender, actions)
                    .await
                    .context("On timeout event".to_string())?;
            }
        }
    }

    // Tcp Disconnect
    let actions = sm.derive_actions(&mut session, config, &Event::TcpDisconnected);
    react(&mut mqtt, &mut timer_sender, actions)
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

async fn react<'a>(
    mqtt: &'a mut TcpStream,
    timer: &mut TimerSender<Pid, ExpectedEvent>,
    actions: Vec<Action<'a>>,
) -> anyhow::Result<()> {
    for action in actions {
        match action {
            Action::Nop => (),
            Action::Send(packet) => {
                println!(">> {packet:?}");
                send_packet(mqtt, packet).await?
            }
            Action::TriggerTimer { pid, expected } => {
                timer.trigger(pid, expected).await;
            }
            Action::ClearTimer { pid } => {
                timer.clear(pid).await;
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

fn message_generator(repeat: Message, interval: Duration) -> mpsc::Receiver<Message> {
    let (tx,rx) = mpsc::channel(10);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if tx.send(repeat.clone()).await.is_err() {
                break;
            }
        }
    });

    rx
}
