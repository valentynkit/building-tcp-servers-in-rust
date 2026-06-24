use std::process::ExitCode;

use building_tcp_servers_in_rust::{Backend, Config, ShutdownSignal};

fn main() -> ExitCode {
    let mut backend = Backend::Blocking;
    let mut port = 9999u16;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--backend" | "-b" => match args.next().map(|s| s.parse::<Backend>()) {
                Some(Ok(b)) => backend = b,
                Some(Err(e)) => return fail(&e),
                None => return fail("--backend needs a value"),
            },
            "--port" | "-p" => match args.next().map(|s| s.parse::<u16>()) {
                Some(Ok(p)) => port = p,
                Some(Err(_)) => return fail("--port needs a number"),
                None => return fail("--port needs a value"),
            },
            "--help" | "-h" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            other => return fail(&format!("unexpected argument: {other}")),
        }
    }

    let cfg = Config {
        port,
        ..Config::default()
    };
    match backend.launch(&cfg, ShutdownSignal::new()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&format!("server error: {e}")),
    }
}

fn print_usage() {
    println!(
        "usage: tcp-server [--backend {}] [--port N]",
        Backend::NAMES.join("|")
    );
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    print_usage();
    ExitCode::FAILURE
}
