use async_std::fs::{self, File};
use async_std::io::{self, BufReader};
use async_std::net::{Shutdown, TcpListener, TcpStream};
use async_std::prelude::*;
use async_std::process::{Command, Stdio};
use async_std::task;
use clap::{App, Arg};
use futures::stream::FuturesUnordered;
use log::{debug, error, info, warn};
use std::env;
use std::sync::Arc;

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const SECRET_LEN: usize = 8;

struct Context {
    secret: String,
}

impl Context {
    fn new(secret: Option<&str>) -> Self {
        Self {
            secret: secret
                .map(|x| x.to_string())
                .unwrap_or(gen_secret(SECRET_LEN)),
        }
    }
}

fn gen_secret(len: usize) -> String {
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    use std::iter;

    let mut rng = thread_rng();
    let chars: String = iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(len)
        .collect();
    chars
}

async fn handshake(ctx: &Context, reader: &mut BufReader<TcpStream>) -> io::Result<()> {
    let mut buffer: Vec<u8> = vec![];
    let len = reader.read_until(b'\n', &mut buffer).await?;
    let line = std::str::from_utf8(&buffer[0..len])
        .map(|s| s.trim())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{}", e)))?;

    debug!("{}", line);

    if line.starts_with("SECRET:") {
        let (_, secret) = line.split_at(7);
        if secret == ctx.secret {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("The secret {} does not matched!", secret),
            ))
        }
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Please provide the `SECRET` first!"),
        ))
    }
}

async fn process(ctx: Arc<Context>, stream: TcpStream) -> io::Result<()> {
    info!("Accepted from: {}", stream.peer_addr()?);

    let reader = stream.clone();
    let writer = stream.clone();
    let mut reader = BufReader::new(reader);

    let _ = handshake(&ctx, &mut reader).await?;

    'outer: loop {
        let mut buffer: Vec<u8> = vec![];
        let len = reader.read_until(b'\n', &mut buffer).await?;
        let line = std::str::from_utf8(&buffer[0..len])
            .map(|s| s.trim())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{}", e)))?;

        debug!("{}", line);

        if line.starts_with("EXEC:") {
            let (_, script) = line.split_at(5);
            let script_args: Vec<&str> = script.split(' ').collect();

            info!("Exec `{}` ...", script);

            let child = Command::new(script_args[0])
                .args(&script_args[1..])
                // .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!(r#"Failed to exec "{}", error: {}"#, script, e),
                    )
                })?;

            let tasks = FuturesUnordered::new();
            let writer1 = writer.clone();
            let writer2 = writer.clone();
            if let Some(stderr) = child.stderr {
                tasks.push(task::spawn(io::copy(stderr, writer1)));
            }
            if let Some(stdout) = child.stdout {
                tasks.push(task::spawn(io::copy(stdout, writer2)));
            }
            for t in tasks {
                if let Err(err) = t.await {
                    warn!("Client Error {:?}", err);
                }
            }
        } else if line.starts_with("EXIT!") {
            break 'outer;
        } else if line.starts_with("FILE:") {
            let (_, last) = line.split_at(5);
            let segs: Vec<&str> = last.split('|').collect();
            let file_name = env::current_dir().map(|x| x.join(&segs[0]))?;
            let length = segs[1]
                .parse::<usize>()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{}", e)))?;
            let mut buffer = Vec::with_capacity(length);
            unsafe {
                buffer.set_len(length);
            }
            let _ = reader.read_exact(&mut buffer).await?;
            let mut file = File::create(&file_name).await?;
            file.write_all(&buffer).await?;
            info!("File `{}` saved", file_name.display());
        } else if line.starts_with("REMOVE:") {
            let (_, file_name) = line.split_at(7);
            let file_name = env::current_dir().map(|x| x.join(file_name))?;
            fs::remove_file(&file_name).await?;
            info!("File `{}` removed", file_name.display());
        } else {
            warn!(r#"Invalid command: "{}""#, line);
            break 'outer;
        }
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let matches = App::new(APP_NAME)
        .version(APP_VERSION)
        .author("Varphone Wong <varphone@qq.com>")
        .about("Simple runner for multiple platforms")
        .arg(
            Arg::with_name("secret")
                .short("s")
                .long("secret")
                .help("Set the secret for verification")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("address")
                .short("a")
                .long("address")
                .help("Set the listen address")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .help("Set the listen port")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("quiet")
                .short("q")
                .long("quiet")
                .multiple(false)
                .help("No output printed to stdout"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .multiple(true)
                .help("Sets the level of verbosity"),
        )
        .get_matches();

    let verbose = if matches.is_present("quiet") {
        "info"
    } else {
        match matches.occurrences_of("verbose") as usize {
            1 => "warn",
            2 => "debug",
            3 => "trace",
            _ => "info",
        }
    };

    env::set_var("RUST_LOG", verbose);
    env_logger::init();

    let ctx = Arc::new(Context::new(matches.value_of("secret")));
    info!("The SECRET is `{}`", ctx.secret);

    task::block_on(async move {
        let listener = TcpListener::bind("0.0.0.0:9677").await?;
        info!("Listening on {}", listener.local_addr()?);

        let mut incoming = listener.incoming();

        while let Some(stream) = incoming.next().await {
            let mut stream = stream?;
            let stream_cloned = stream.clone();
            let ctx_cloned = Arc::clone(&ctx);
            task::spawn(async move {
                if let Err(e) = process(ctx_cloned, stream_cloned).await {
                    error!("{}", e);
                    let msg = format!("RUNNER: {}\n", e);
                    let _ = stream.write_all(msg.as_bytes()).await;
                }
                let _ = stream.shutdown(Shutdown::Both);
            });
        }
        Ok(())
    })
}
