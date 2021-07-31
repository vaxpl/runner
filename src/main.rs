use async_std::fs::{self, File};
use async_std::io::{self, BufReader};
use async_std::net::{Shutdown, TcpListener, TcpStream};
use async_std::prelude::*;
use async_std::process::{Command, Stdio};
use async_std::task::{self, JoinHandle};
use clap::{App, AppSettings, Arg, SubCommand};
use futures::stream::FuturesUnordered;
use log::{debug, error, info, warn};
use std::env;
use std::path::Path;
use std::sync::Arc;

const ACCESS_TOKEN_LEN: usize = 8;
const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_ENVIRONMENTS: &str = r#"ENVIRONMENT:
    RUNNER_HOST      Set the listen or connect address, same -a|--address
    RUNNER_PORT      Set the listen or connect port, same -p|--port
    RUNNER_TOKEN     Set the access token for security authentication, same -t|--token
"#;

macro_rules! other_err {
    ($expr:expr) => {
        io::Error::new(io::ErrorKind::Other, $expr)
    };
}

#[derive(Debug)]
struct Config {
    address: String,
    port: u16,
    token: String,
}

impl Config {
    fn new() -> Self {
        Self {
            address: "127.0.0.1".to_owned(),
            port: 9677,
            token: gen_token(ACCESS_TOKEN_LEN),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

fn gen_token(len: usize) -> String {
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

async fn handshake(cfg: &Config, reader: &mut BufReader<TcpStream>) -> io::Result<()> {
    let mut buffer: Vec<u8> = vec![];
    let len = reader.read_until(b'\n', &mut buffer).await?;
    let line = std::str::from_utf8(&buffer[0..len])
        .map(|s| s.trim())
        .map_err(|e| other_err!(format!("{}", e)))?;

    debug!("{}", line);

    if line.starts_with("TOKEN:") {
        let (_, token) = line.split_at(6);
        if token == cfg.token {
            Ok(())
        } else {
            Err(other_err!(format!(
                "The access token {} does not matched!",
                token
            )))
        }
    } else {
        Err(other_err!(format!("Please provide the `TOKEN` first!")))
    }
}

async fn process(cfg: &Config, stream: TcpStream) -> io::Result<()> {
    info!("Accepted from: {}", stream.peer_addr()?);

    let reader = stream.clone();
    let writer = stream.clone();
    let mut reader = BufReader::new(reader);

    let _ = handshake(&cfg, &mut reader).await?;

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

    println!("Finished");

    Ok(())
}

fn run(cfg: Config, args: Vec<String>) -> io::Result<()> {
    debug!("runnnnnnnnnnnnnnnnnnn");
    task::block_on(async move {
        // Connect to runner server
        let stream = TcpStream::connect((cfg.address.as_str(), cfg.port)).await?;
        let mut reader = stream.clone();
        let mut writer = stream.clone();

        let t1 = task::spawn(io::copy(reader, io::stdout()));
        let t2: JoinHandle<io::Result<()>> = task::spawn(async move {
            // Send access token first
            let msg = format!("TOKEN:{}\n", cfg.token);
            writer.write_all(msg.as_bytes()).await?;
            // Strip the executable file directory
            let name = Path::new(&args[0])
                .file_name()
                .map(|x| x.to_str())
                .flatten()
                .ok_or(other_err!("akkkkkk"))?;
            let exec_cmd = format!("EXEC:{} {}\n", name, &args[1..].join(" "));
            // Send the executable file if exists
            if let Ok(meta) = fs::metadata(&args[0]).await {
                if meta.len() > 0 {
                    // Send information of the file
                    let file_cmd = format!("FILE:{}|{}\n", name, meta.len());
                    writer.write_all(file_cmd.as_bytes()).await?;
                    // Send the content of the file
                    let mut file = File::open(&args[0]).await?;
                    io::copy(&mut file, &mut writer).await?;
                    // Execute the uploaded file
                    writer.write_all(exec_cmd.as_bytes()).await?;
                    // Remove the uploaded file
                    let remove_cmd = format!("REMOVE:{}\n", name);
                    writer.write_all(remove_cmd.as_bytes()).await?;
                }
            } else {
                writer.write_all(exec_cmd.as_bytes()).await?;
            }
            // Shutdown the connection
            writer.write_all(b"EXIT!\n").await?;
            Ok(())
        });

        t1.await.unwrap();
        t2.await.unwrap();
        Ok(())
    })
}

fn server(cfg: Config) -> io::Result<()> {
    let cfg = Arc::new(cfg);

    info!("The access token is `{}`", cfg.token);

    task::block_on(async move {
        let listener = TcpListener::bind((cfg.address.as_str(), cfg.port)).await?;
        info!("Listening on {}", listener.local_addr()?);

        let mut incoming = listener.incoming();

        while let Some(stream) = incoming.next().await {
            let mut stream = stream?;
            let stream_cloned = stream.clone();
            let cfg_cloned = Arc::clone(&cfg);
            task::spawn(async move {
                if let Err(e) = process(&cfg_cloned, stream_cloned).await {
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

fn main() -> io::Result<()> {
    let matches = App::new(APP_NAME)
        .version(APP_VERSION)
        .author("Varphone Wong <varphone@qq.com>")
        .about("Simple runner for multiple platforms")
        .arg(
            Arg::with_name("address")
                .env("RUNNER_HOST")
                .short("a")
                .long("address")
                .help("Set the listen or connect address")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .env("RUNNER_PORT")
                .short("p")
                .long("port")
                .help("Set the listen or connect port")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("token")
                .env("RUNNER_TOKEN")
                .short("t")
                .long("token")
                .help("Set the access token for security authentication")
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
        .subcommand(
            SubCommand::with_name("run")
                .about("Send the PROG to the runner server and run it")
                .setting(AppSettings::TrailingVarArg)
                .arg(
                    Arg::with_name("PROG")
                        .help("The executable to send and run")
                        .index(1)
                        .multiple(true)
                        .required(true),
                ),
        )
        .subcommand(SubCommand::with_name("server").about("Run as a runner server"))
        .after_help(APP_ENVIRONMENTS)
        .get_matches();

    let verbose = if matches.is_present("quiet") {
        "info"
    } else {
        match matches.occurrences_of("verbose") as usize {
            0 => "info",
            1 => "warn",
            2 => "debug",
            _ => "trace",
        }
    };

    env::set_var("RUST_LOG", verbose);
    env_logger::init();

    let mut cfg = Config::new();
    if let Some(v) = matches
        .value_of("address")
        .map(str::to_owned)
        .or(env::var("RUNNER_HOST").ok())
    {
        cfg.address = v;
    }
    if let Some(v) = matches
        .value_of("port")
        .map(str::to_owned)
        .or(env::var("RUNNER_PORT").ok())
    {
        cfg.port = v.parse::<u16>().map_err(|e| other_err!(format!("{}", e)))?;
    }
    if let Some(v) = matches
        .value_of("token")
        .map(str::to_owned)
        .or(env::var("RUNNER_TOKEN").ok())
    {
        cfg.token = v;
    }

    match matches.subcommand() {
        ("run", Some(sub_matches)) => {
            let args: Vec<String> = sub_matches
                .values_of("PROG")
                .unwrap()
                .map(str::to_owned)
                .collect();
            run(cfg, args)?;
        }
        ("server", Some(sub_matches)) => {
             server(cfg)?;
        }
        (u, v) => {
            debug!("u={:?}, v={:?}", u, v);
            // Err(io::Error::new(io::ErrorKind::Unsupported, "Aka"))
        }
    }

    Ok(())
}
