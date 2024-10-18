use arboard::{Clipboard, ImageData};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt::Display, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{ReadHalf, WriteHalf},
        TcpListener, TcpStream,
    },
};

#[derive(Serialize, Deserialize)]
enum CbData {
    Text(String),
    Image(#[serde(with = "ImageDataDef")] ImageData<'static>),
}

impl std::fmt::Display for CbData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            CbData::Text(str) => f.write_str(str),
            CbData::Image(img) => f.write_str(&format!("Image {}x{}", img.width, img.height)),
        }
    }
}

impl PartialEq for CbData {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(l0), Self::Text(r0)) => l0 == r0,
            (Self::Image(l0), Self::Image(r0)) => {
                l0.width == r0.width && l0.height == r0.height && l0.bytes == r0.bytes
            }
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ImageData")]
struct ImageDataDef<'a> {
    pub width: usize,
    pub height: usize,
    pub bytes: Cow<'a, [u8]>,
}

async fn handle_output(peer: impl Display, mut stream: WriteHalf<'_>) -> anyhow::Result<()> {
    let mut clip = Clipboard::new().unwrap();
    let mut last_cb_data = None;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let new_cb_data = if let Ok(text) = clip.get_text() {
            Some(CbData::Text(text))
        } else if let Ok(img) = clip.get_image() {
            Some(CbData::Image(img))
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

async fn handle_input(peer: impl Display, mut stream: ReadHalf<'_>) -> anyhow::Result<()> {
    let mut clip = Clipboard::new().unwrap();
    loop {
        let mut buf = vec![];
        let x = loop {
            let n = stream.read_buf(&mut buf).await?;
            if matches!(n, 0) {
                return Ok(());
            }
            match postcard::from_bytes::<CbData>(&buf) {
                Ok(x) => break x,
                Err(postcard::Error::DeserializeUnexpectedEnd) => {
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
        };
        log::trace!("Got clipboard from {}: {x}", peer);
        match x {
            CbData::Text(text) => {
                if let Err(e) = clip.set_text(text) {
                    log::error!("{e}");
                }
            }
            CbData::Image(image_data) => {
                if let Err(e) = clip.set_image(image_data) {
                    log::error!("{e}");
                }
            }
        }
    }
}

#[derive(clap::Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Cmd,
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

async fn handle_client(peer: impl Display, mut stream: TcpStream) {
    let (read, write) = stream.split();
    let err = tokio::select! {
        err = handle_input(&peer, read) => {
            log::info!("handle_input for {peer} terminated");
            err
        },
        err = handle_output(&peer, write) => {
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
                        tasks.spawn(handle_client(peer, stream));
                    }
                    Err(e) => log::error!("{e}"),
                }
            }
        }
        Cmd::Client { host } => {
            let stream = TcpStream::connect(&host).await.unwrap();
            log::info!("Connected to {host}");
            handle_client(host, stream).await;
        }
    }
}
