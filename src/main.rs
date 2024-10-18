use arboard::{Clipboard, ImageData};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    io::BufWriter,
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

#[derive(Serialize, Deserialize, Debug)]
enum CbData {
    Text(String),
    Image(#[serde(with = "ImageDataDef")] ImageData<'static>),
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

fn handle_output(terminated: Arc<AtomicBool>, mut stream: TcpStream) {
    let mut clip = Clipboard::new().unwrap();
    let mut last_cb_data = None;
    let peer = stream.peer_addr().unwrap();
    let mut buf = BufWriter::new(&mut stream);
    while !terminated.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(500));
        let new_cb_data = if let Ok(text) = clip.get_text() {
            Some(CbData::Text(text))
        } else if let Ok(img) = clip.get_image() {
            Some(CbData::Image(img))
        } else {
            None
        };
        if new_cb_data != last_cb_data {
            log::trace!("Sending {new_cb_data:?} to {peer}");
            last_cb_data = new_cb_data;
            if let Some(data) = &last_cb_data {
                if let Err(e) = postcard::to_io(data, &mut buf) {
                    log::error!("{e}");
                    terminated.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    }
}

fn handle_input(terminated: Arc<AtomicBool>, mut stream: TcpStream) {
    let mut clip = Clipboard::new().unwrap();
    let peer = stream.peer_addr().unwrap();
    let mut buf = vec![0; 1024 * 1024]; // 1M
    loop {
        match postcard::from_io::<CbData, _>((&mut stream, buf.as_mut_slice())) {
            // silly workaround for a closed connection
            Ok((CbData::Text(str), _)) if str.is_empty() => {
                terminated.store(true, Ordering::Relaxed);
                break;
            }
            Ok((n, _)) => match n {
                CbData::Text(text) => {
                    log::trace!("Got clipboard text: {text} from {}", peer);
                    if let Err(e) = clip.set_text(text) {
                        log::error!("{e}");
                    }
                }
                CbData::Image(image_data) => {
                    log::trace!("Got clipboard image from {}", peer);
                    if let Err(e) = clip.set_image(image_data) {
                        log::error!("{e}");
                    }
                }
            },
            Err(err) => {
                terminated.store(true, Ordering::Relaxed);
                log::error!("{err}");
                break;
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
        #[clap(short = 'p', default_value_t = 1234)]
        port: u16,
    },
    Client {
        host: String,
    },
}

fn main() {
    pretty_env_logger::init();
    let args = Args::parse();
    match args.command {
        Cmd::Server { port } => {
            let listener = TcpListener::bind(format!("0.0.0.0:{port}")).unwrap();

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let terminated = Arc::new(AtomicBool::new(false));
                        thread::spawn({
                            let stream = stream.try_clone().unwrap();
                            let terminated = terminated.clone();
                            move || {
                                handle_output(terminated, stream);
                            }
                        });
                        thread::spawn(move || {
                            handle_input(terminated, stream);
                        });
                    }
                    Err(e) => log::error!("{e}"),
                }
            }
        }
        Cmd::Client { host } => {
            let stream = TcpStream::connect(host).unwrap();
            let terminated = Arc::new(AtomicBool::new(false));
            thread::spawn({
                let stream = stream.try_clone().unwrap();
                let terminated = terminated.clone();
                move || {
                    handle_output(terminated, stream);
                }
            });
            handle_input(terminated, stream);
        }
    }
}
