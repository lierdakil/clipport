mod clipboard;

use clap::Parser;
use derive_more::derive::From;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{ReadHalf, WriteHalf},
        TcpListener, TcpStream,
    },
};

use self::clipboard::{Clipboard, Image, WaylandClipboard};

#[derive(Serialize, Deserialize, PartialEq, From)]
enum CbData {
    Text(String),
    Image(Image<'static>),
}

impl std::fmt::Display for CbData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            CbData::Text(str) => f.write_str(str),
            CbData::Image(img) => f.write_str(&format!("Image {}x{}", img.width, img.height)),
        }
    }
}

async fn handle_output(
    peer: impl Display,
    mut stream: WriteHalf<'_>,
    wayland: bool,
) -> anyhow::Result<()> {
    let mut clip: Box<dyn Clipboard + Send> = if wayland {
        Box::new(WaylandClipboard)
    } else {
        Box::new(arboard::Clipboard::new().unwrap())
    };
    let mut last_cb_data: Option<CbData> = None;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let new_cb_data = if let Ok(text) = clip.get_text().inspect_err(log) {
            Some(text.into())
        } else if let Ok(img) = clip.get_image().inspect_err(log) {
            Some(img.into())
        } else {
            None
        };
        if new_cb_data != last_cb_data {
            last_cb_data = new_cb_data;
            if let Some(data) = &last_cb_data {
                log::trace!("Sending to {peer}: {data}");
                stream.write_all(&postcard::to_stdvec(data)?).await?;
                stream.flush().await?
            }
        }
    }
}

async fn handle_input(
    peer: impl Display,
    mut stream: ReadHalf<'_>,
    wayland: bool,
) -> anyhow::Result<()> {
    let mut clip: Box<dyn Clipboard + Send> = if wayland {
        Box::new(WaylandClipboard)
    } else {
        Box::new(arboard::Clipboard::new().unwrap())
    };
    let mut buf = vec![];
    loop {
        let x = loop {
            let n = stream.read_buf(&mut buf).await?;
            if matches!(n, 0) {
                return Ok(());
            }
            match postcard::take_from_bytes::<CbData>(&buf) {
                Ok((x, rest)) => {
                    buf = rest.to_vec();
                    break x;
                }
                Err(postcard::Error::DeserializeUnexpectedEnd) => {
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
        };
        log::trace!("Got clipboard from {}: {x}", peer);
        match x {
            CbData::Text(text) => {
                if clip.get_text().inspect_err(log).ok().as_ref() != Some(&text) {
                    if let Err(e) = clip.set_text(text) {
                        log::error!("{e}");
                    }
                }
            }
            CbData::Image(image_data) => {
                if clip.get_image().inspect_err(log).ok().as_ref() != Some(&image_data) {
                    if let Err(e) = clip.set_image(image_data) {
                        log::error!("{e}");
                    }
                }
            }
        }
    }
}

fn log(err: &impl std::fmt::Display) {
    log::debug!("Error: {err}");
}

#[derive(clap::Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Cmd,
    #[arg(long)]
    wayland: bool,
}

#[derive(clap::Subcommand, Clone)]
enum Cmd {
    Server {
        #[clap(short = 'p', default_value_t = 5563)]
        port: u16,
    },
    Client {
        host: String,
    },
}

async fn handle_client(peer: impl Display, mut stream: TcpStream, wayland: bool) {
    let (read, write) = stream.split();
    let err = tokio::select! {
        err = handle_input(&peer, read, wayland) => {
            log::info!("handle_input for {peer} terminated");
            err
        },
        err = handle_output(&peer, write, wayland) => {
            log::info!("handle_output for {peer} terminated");
            err
        }
    };
    if let Err(e) = err {
        log::error!("{e}");
    }
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    let args = Args::parse();
    let mut tasks = tokio::task::JoinSet::new();
    match args.command {
        Cmd::Server { port } => {
            let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();

            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        log::info!("New connection from {peer}");
                        tasks.spawn(handle_client(peer, stream, args.wayland));
                    }
                    Err(e) => log::error!("{e}"),
                }
            }
        }
        Cmd::Client { host } => {
            let stream = TcpStream::connect(&host).await.unwrap();
            log::info!("Connected to {host}");
            handle_client(host, stream, args.wayland).await;
        }
    }
}
