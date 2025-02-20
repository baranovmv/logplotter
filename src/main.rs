use clap::{Parser, ValueHint};
use std::fs::File;
use std::fs::metadata;
use std::io::{Seek, SeekFrom};
use std::ops::{Add, AddAssign};
use std::os::linux::raw::stat;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::sleep;
use std::time;
use ctrlc;
use std::time::{Duration, SystemTime};
use anyhow::{anyhow, Result};
use yaml_rust2::{YamlLoader, Yaml};
// use axum::{routing::get_service, Router};
// use hyper::server;
// use std::{net::SocketAddr, path::PathBuf};
// use tower_http::services::ServeDir;
use tokio;
use warp::Filter;


mod utils;
mod logrecord;

use utils::line_reader::*;
use utils::Once;
use crate::logrecord::LogRecordType;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short='i', help = "the input log file", value_hint = ValueHint::FilePath)]
    log_file: std::path::PathBuf,

    #[arg(short='c', help = "Config file in yaml", value_hint = ValueHint::FilePath)]
    config_file: std::path::PathBuf,
}



#[tokio::main]
async fn main() -> Result<()> {
    const PRINT_PERIOD: u16 = 5;
    let args = Args::parse();
    if !args.log_file.is_file() {
        return Err(anyhow!("{} is not a file", args.log_file.display()))
    }
    let mut recordstypes = load_config(&args.config_file)?;

    let ctrlc = Arc::new(AtomicBool::new(false));
    let ctrlc_for_signal = ctrlc.clone();
    ctrlc::set_handler(move || {
        ctrlc_for_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    tokio::spawn(serve_http());

    let mut log_file = File::open(&args.log_file)?;
    let mut last_size = log_file.seek(SeekFrom::End(0))?;
    let mut remainder = String::new();

    let mut line_count: u16 = 0;
    let mut parsed_line_count: u16 = 0;
    let mut print_timer = Once::new(time::Duration::from_secs(PRINT_PERIOD.into()));
    loop {
        if ctrlc.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let lines = log_file.incremental_read_line(&mut remainder)?;
        if lines.is_empty() {
            tokio::time::sleep(Duration::from_millis(50)).await;
            // Get file metadata
            if let Ok(meta) = metadata(&args.log_file) {
                let new_size = meta.len();

                if new_size < last_size {
                    eprintln!("File truncated. Resetting reader...");
                    log_file = File::open(&args.log_file)?;
                    last_size = 0;
                }

                last_size = new_size;
            }
            continue;
        }
        for records in recordstypes.iter_mut() {
            parsed_line_count.add_assign(u16::try_from(records.parse(&lines))?);
        }

        line_count.add_assign(u16::try_from(lines.len())?);
        if print_timer.once() {
            let lines_per_sec = f32::try_from(line_count)? / f32::from(PRINT_PERIOD);
            let parsed_per_sec = f32::try_from(parsed_line_count)? / f32::from(PRINT_PERIOD);
            parsed_line_count = 0;
            print!("{lines_per_sec:.1} lines per second\t{parsed_per_sec}\n");

            // line_count -= round(lines_per_sec * PRINT_PERIOD)
            line_count = line_count.saturating_sub(
                f32::round(lines_per_sec * f32::from(PRINT_PERIOD)) as u16
            );
        }
    }

    println!("Finish");
    Ok(())
}


fn load_config(file_path: &std::path::PathBuf) -> anyhow::Result<Vec<LogRecordType>> {
    if !file_path.is_file() {
        return Err(anyhow!("{} is not a file", file_path.display()))
    }

    let content = std::fs::read_to_string(file_path)?;
    let docs = YamlLoader::load_from_str(&content)
        .or_else(|e| Err(anyhow!("Error parsing YAML file: {}", e.info())))?;

    let mut recordstypes = Vec::<LogRecordType>::new();
    for doc in docs {
        if let Some(hash) = doc.as_hash() {
            for (name, record_settings) in hash.iter() {
                let Some(name_s) = name.as_str() else { continue };
                let Some(regex) = record_settings["regex"].as_str() else { continue };
                recordstypes.push(LogRecordType::new(regex)?);
                parse_plots(&record_settings["plots"], recordstypes.last_mut().unwrap());
            }
        }
    }

    Ok(recordstypes)
}

fn parse_plots(plots_yml: &Yaml, conf: &mut LogRecordType) {
    for (plot_name, plot_conf) in plots_yml.as_hash().unwrap().iter() {
        let Some(name_s) = plot_name.as_str() else {continue};
        let axis = plot_conf["axis"].as_i64().and_then(|x| u8::try_from(x).ok());
        let style = plot_conf["style"].as_str();
        conf.add_field(&name_s, axis, style);
    }
}

async fn serve_http(channel_rx: ) {

    // GET /hello/from/warp
    let status = warp::path!("hello" / "from" / "warp").map(|| "Hello from warp!");

    let route = warp::get()
        .and(warp::fs::dir("static/").
            or(status)
        );

    warp::serve(route)
        .run(([127, 0, 0, 1], 3030))
        .await;
}
