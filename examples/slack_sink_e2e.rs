//! Run Rent-ie-notify end-to-end against a local Slack webhook sink.
//!
//! The program still fetches the configured rent.ie/daft.ie sources, but Slack
//! POSTs go to a temporary localhost server instead of a real webhook. The
//! program runs in a temporary directory, so the real `sent_ids.txt` is untouched.

use std::{
    env,
    error::Error,
    fs,
    net::TcpListener,
    path::PathBuf,
    process::{Command, ExitCode},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use nanoserde::DeJson;
use tempfile::tempdir;
use tiny_http::{Response, Server};

const USAGE: &str = "\
Usage: cargo slack-sink-e2e [OPTIONS]

The binary under test is run with SLACK_WEBHOOK_URL pointed at a local sink
and a temporary working directory for sent_ids.txt.

Options:
  --rent-url URL              rent.ie search URL
  --daft-request JSON         daft.ie gateway JSON request body
  --daft-request-file PATH    read the daft.ie request body from a file
  --binary PATH               binary under test
  --with-routing              forward OPENROUTESERVICE_API_KEY to enable ORS calls
  -h, --help                  show this help

Defaults come from RENT_IE_URL and DAFT_IE_REQUEST.
At least one source must be configured. OpenRouteService is disabled unless
--with-routing is passed, so test runs do not consume map API requests.
";

#[derive(Debug, DeJson)]
struct SlackPayload {
    blocks: Vec<SlackBlock>,
}

#[derive(Debug, DeJson)]
struct SlackBlock {
    #[nserde(rename = "type")]
    block_type: String,
}

struct Config {
    binary: PathBuf,
    rent_url: Option<String>,
    daft_request: Option<String>,
    with_routing: bool,
}

fn default_binary() -> Result<PathBuf, Box<dyn Error>> {
    let current_exe = env::current_exe()?;
    let examples_dir = current_exe
        .parent()
        .ok_or_else(|| "failed to locate example directory".to_string())?;
    let profile_dir = examples_dir
        .parent()
        .ok_or_else(|| "failed to locate target profile directory".to_string())?;
    let binary_name = format!("Rent-ie-notify{}", env::consts::EXE_SUFFIX);

    Ok(profile_dir.join(binary_name))
}

fn parse_args() -> Result<Option<Config>, Box<dyn Error>> {
    let mut binary = default_binary()?;
    let mut rent_url = env::var("RENT_IE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let mut daft_request = env::var("DAFT_IE_REQUEST")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let mut with_routing = false;
    let mut args = env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--binary" => {
                binary = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--binary requires a path".to_string())?,
                );
            }
            "--rent-url" => {
                rent_url = Some(
                    args.next()
                        .ok_or_else(|| "--rent-url requires a URL".to_string())?,
                );
            }
            "--daft-request" => {
                daft_request = Some(
                    args.next()
                        .ok_or_else(|| "--daft-request requires a JSON body".to_string())?,
                );
            }
            "--daft-request-file" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--daft-request-file requires a path".to_string())?;
                let body = fs::read_to_string(&path)
                    .map_err(|error| format!("failed to read {path}: {error}"))?;
                daft_request = Some(body.trim().to_string());
            }
            "--with-routing" => with_routing = true,
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    if !binary.is_file() {
        return Err(format!(
            "binary not found: {} (run cargo build --locked)",
            binary.display()
        )
        .into());
    }

    Ok(Some(Config {
        binary,
        rent_url,
        daft_request,
        with_routing,
    }))
}

fn summarize_slack_payload(body: &[u8]) -> Option<(usize, usize)> {
    let text = std::str::from_utf8(body).ok()?;
    let payload = SlackPayload::deserialize_json(text).ok()?;
    let listings = payload
        .blocks
        .iter()
        .filter(|block| block.block_type == "section")
        .count();

    Some((payload.blocks.len(), listings))
}

fn run_slack_sink(
    received: &Arc<Mutex<Vec<Vec<u8>>>>,
    shutdown: &Arc<AtomicBool>,
) -> Result<(u16, thread::JoinHandle<()>), Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let server = Server::from_listener(listener, None)
        .map_err(|error| format!("failed to start Slack sink: {error}"))?;
    let server = Arc::new(server);
    let thread_server = Arc::clone(&server);
    let thread_received = Arc::clone(received);
    let thread_shutdown = Arc::clone(shutdown);

    let handle = thread::spawn(move || {
        while !thread_shutdown.load(Ordering::Relaxed) {
            match thread_server.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(mut request)) => {
                    let mut body = Vec::new();
                    if request.as_reader().read_to_end(&mut body).is_ok()
                        && let Ok(mut received) = thread_received.lock()
                    {
                        received.push(body);
                    }
                    let _ = request.respond(Response::from_string("ok").with_status_code(200));
                }
                Ok(None) => {}
                Err(_) => break,
            }
        }
    });

    Ok((port, handle))
}

fn run() -> Result<bool, Box<dyn Error>> {
    let Some(config) = parse_args()? else {
        print!("{USAGE}");
        return Ok(true);
    };

    if config.rent_url.is_none() && config.daft_request.is_none() {
        return Err(
            "set RENT_IE_URL and/or DAFT_IE_REQUEST, or pass --rent-url/--daft-request-file".into(),
        );
    }

    let routing_api_key = if config.with_routing {
        Some(
            env::var("OPENROUTESERVICE_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or("--with-routing requires OPENROUTESERVICE_API_KEY")?,
        )
    } else {
        None
    };

    let received = Arc::new(Mutex::new(Vec::new()));
    let shutdown = Arc::new(AtomicBool::new(false));
    let (port, server_thread) = run_slack_sink(&received, &shutdown)?;

    let workdir = tempdir()?;
    let mut command = Command::new(&config.binary);
    command
        .current_dir(workdir.path())
        .env(
            "SLACK_WEBHOOK_URL",
            format!("http://127.0.0.1:{port}/webhook"),
        )
        .env_remove("RENT_IE_URL")
        .env_remove("DAFT_IE_REQUEST")
        .env_remove("OPENROUTESERVICE_API_KEY");

    if let Some(rent_url) = &config.rent_url {
        command.env("RENT_IE_URL", rent_url);
    }
    if let Some(daft_request) = &config.daft_request {
        command.env("DAFT_IE_REQUEST", daft_request);
    }
    if let Some(api_key) = &routing_api_key {
        command.env("OPENROUTESERVICE_API_KEY", api_key);
    }

    let output = command.output()?;

    shutdown.store(true, Ordering::Relaxed);
    if server_thread.join().is_err() {
        eprintln!("Slack sink thread panicked");
    }

    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));

    let sent_ids_path = workdir.path().join("sent_ids.txt");
    if let Ok(content) = fs::read_to_string(sent_ids_path) {
        let ids: Vec<&str> = content.lines().filter(|line| !line.is_empty()).collect();
        let first_id = ids.first().copied().unwrap_or("none");
        println!("Saved IDs: {} (first: {first_id})", ids.len());
    }

    let bodies = received
        .lock()
        .map_err(|_| "Slack sink mutex poisoned".to_string())?;
    println!("Slack requests: {}", bodies.len());
    for (index, body) in bodies.iter().enumerate() {
        match summarize_slack_payload(body) {
            Some((blocks, listings)) => {
                println!(
                    "  message {}: {blocks} blocks, {listings} listings",
                    index + 1
                );
            }
            None => println!("  message {}: {} bytes (not JSON)", index + 1, body.len()),
        }
    }
    drop(bodies);

    Ok(output.status.success())
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}
