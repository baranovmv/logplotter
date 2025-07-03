use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::fs::metadata;
use std::io::{Seek, SeekFrom};
use std::net::SocketAddr;
use std::ops::{Add, AddAssign};
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time;
use clap::{Parser, ValueHint};
use ctrlc;
use std::time::{Duration, SystemTime};
use anyhow::{anyhow, Result};
use yaml_rust2::{YamlLoader, Yaml};
use poem::{
    get,
    handler,
    listener::{Listener, TcpListener},
    endpoint::StaticFilesEndpoint,
    Route, Server,
    middleware::Cors,
    web::{Json, Query},
    EndpointExt
};
use rand::{SeedableRng, rngs::SmallRng, RngCore};
use serde::Serialize;
use tokio;

mod utils;
mod logrecord;

use utils::line_reader::*;
use utils::Once;
use crate::logrecord::*;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short='i', help = "the input log file", value_hint = ValueHint::FilePath)]
    log_file: std::path::PathBuf,

    #[arg(short='c', help = "Config file in yaml", value_hint = ValueHint::FilePath)]
    config_file: std::path::PathBuf,

    #[arg(short='t', help = "Maximum length of stored history in seconds")]
    max_hist_len: Option<f64>,

    #[arg(short='s', help = "Static")]
    stat: Option<bool>,
}

// Shared state across threads
type SharedStreams = Arc<Mutex<VecDeque<ParsedBlock>>>;
type ClientTracker = Arc<Mutex<HashMap<String, f64>>>;

#[tokio::main]
async fn main() -> Result<()> {
    const PRINT_PERIOD: u16 = 5;
    let args = Args::parse();
    if !args.log_file.is_file() {
        return Err(anyhow!("{} is not a file", args.log_file.display()))
    }
    let recordstypes = Arc::new(load_config(&args.config_file)?);
    let recodsconfig = recordstypes.to_json()?;
    let max_parsed_list_len = args.max_hist_len.unwrap_or(10f64);
    let mut parser = LogParser::new(recordstypes.clone());

    let ctrlc = Arc::new(AtomicBool::new(false));
    let ctrlc_for_signal = ctrlc.clone();
    ctrlc::set_handler(move || {
        ctrlc_for_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    let parsed_block_list = Arc::new(Mutex::new(VecDeque::<logrecord::ParsedBlock>::new()));
    let state = parsed_block_list.clone();
    let client_tracker = Arc::new(Mutex::new(HashMap::<String, f64>::new()));
    tokio::spawn(async move {
        let app = Route::new().nest(
        "/",
            StaticFilesEndpoint::new("./static/").index_file("index.html"),
        );
        let cors = Cors::new()
            .allow_origin("*")
            .allow_methods(vec!["GET", "POST", "OPTIONS"])
            .allow_headers(vec!["Content-Type"])
            .expose_headers(vec!["Access-Control-Allow-Origin"])
            .allow_credentials(false)
            .max_age(86400); // Cache preflight responses
        let app = Route::new()
            .at("/data", get(get_data.data(state).data(client_tracker)))
            .at("/config", get(get_config.data(recodsconfig)))
            .nest("/", StaticFilesEndpoint::new("static").index_file("index.html"))
            .with(cors);
        Server::new(TcpListener::bind("0.0.0.0:3000"))
            .run(app)
            .await.unwrap()
    });

    let mut log_file = File::open(&args.log_file)?;
    let mut last_size = if args.stat.unwrap_or(false) { 0 } else { log_file.seek(SeekFrom::End(0))? };
    let mut remainder = Vec::new();

    let mut line_count: u32 = 0;
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
        if let Some((parse_result, cur_parsed_line)) = parser.parse(&lines) {
            let mut parsed_blocks_list = parsed_block_list.lock().unwrap();
            parsed_line_count.add_assign(u16::try_from(cur_parsed_line)?);
            parsed_blocks_list.push_front(parse_result);
            loop {
                let hist_len = parsed_blocks_list.front().unwrap().get_ts()
                    - parsed_blocks_list.back().unwrap().get_ts();
                if hist_len > max_parsed_list_len {
                    parsed_blocks_list.pop_back();
                } else {
                    break;
                }
            }
        } else { continue; }

        line_count.add_assign(u32::try_from(lines.len())?);
        if print_timer.once() {
            let lines_per_sec = f64::try_from(line_count)? / f64::from(PRINT_PERIOD);
            let parsed_per_sec = f32::try_from(parsed_line_count)? / f32::from(PRINT_PERIOD);
            parsed_line_count = 0;
            print!("{lines_per_sec:.1} lines per second\t{parsed_per_sec}\n");

            // line_count -= round(lines_per_sec * PRINT_PERIOD)
            line_count = line_count.saturating_sub(
                f64::round(lines_per_sec * f64::from(PRINT_PERIOD)) as u32
            );
        }
    }

    println!("Finish");
    drop(parser);
    Ok(())
}


fn load_config(file_path: &std::path::PathBuf) -> anyhow::Result<LogRecordsConfig> {
    if !file_path.is_file() {
        return Err(anyhow!("{} is not a file", file_path.display()))
    }

    let content = std::fs::read_to_string(file_path)?;
    let docs = YamlLoader::load_from_str(&content)
        .or_else(|e| Err(anyhow!("Error parsing YAML file: {}", e.info())))?;

    let mut recordstypes = LogRecordsConfig::new();
    for doc in docs {
        if let Some(hash) = doc.as_hash() {
            for (name, record_settings) in hash.iter() {
                let Some(name_s) = name.as_str() else { continue };
                let Some(regex) = record_settings["regex"].as_str() else { continue };
                let mut record_type = LogRecordType::new(name_s, regex)?;
                parse_plots(&record_settings["plots"], &mut record_type);
                recordstypes.insert(name_s.to_string(), record_type);
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
        let coef = plot_conf["coef"].as_f64();
        conf.add_field(&name_s, axis, style, coef);
    }
}

#[handler]
async fn get_data(
    parsed_block_list: poem::web::Data<&SharedStreams>,
    client_tracker: poem::web::Data<&ClientTracker>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<Vec<ParsedBlock>> {
    let client_id = params.get("client_id").unwrap_or(&"unknown".to_string()).clone();
    let last_seen = {
        let mut tracker = client_tracker.lock().unwrap();
        tracker.entry(client_id.clone()).or_insert(0f64).clone()
    };

    let streams = {
        let parsedblocks = parsed_block_list.lock().unwrap();
        // println!("Available {}", parsedblocks.len());
        let mut filtered_streams: Vec<ParsedBlock> = parsedblocks.iter()
                                            .filter(move |&x| x.get_ts() > last_seen)
                                            .map(|x| x.clone()).rev().collect();
        filtered_streams
    };

    // Update client's last seen ID
    let min_ts = streams.iter().map(|s| s.get_ts()).reduce(f64::min).unwrap_or(f64::NAN);
    let max_ts = streams.iter().map(|s| s.get_ts()).reduce(f64::max).unwrap_or(f64::NAN);
    if !max_ts.is_nan() {
        let mut tracker = client_tracker.lock().unwrap();
        tracker.insert(client_id.clone(), max_ts.clone());
    }

    // println!("Feeded {client_id} with {} blocks {min_ts}:{max_ts}\n", streams.len());;

    Json( streams )
}

#[handler]
async fn get_config(config: poem::web::Data<&String>)
    -> String
{
    config.clone()
}