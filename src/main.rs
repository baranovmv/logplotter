use std::collections::HashMap;
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
    web::Json,
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
}



#[tokio::main]
async fn main() -> Result<()> {
    const PRINT_PERIOD: u16 = 5;
    let args = Args::parse();
    if !args.log_file.is_file() {
        return Err(anyhow!("{} is not a file", args.log_file.display()))
    }
    let recordstypes = Arc::new(load_config(&args.config_file)?);
    let parser = LogParser::new(recordstypes.clone());

    let ctrlc = Arc::new(AtomicBool::new(false));
    let ctrlc_for_signal = ctrlc.clone();
    ctrlc::set_handler(move || {
        ctrlc_for_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    let parsed_data_store = Arc::new(Mutex::<LogFields>::new(LogFields::new()));
    let state = AppState {
        result_list: parsed_data_store.clone(),
        conf: recordstypes
    };
    tokio::spawn(async move {
        let app = Route::new().nest(
        "/",
            StaticFilesEndpoint::new("./static/").index_file("index.html"),
        );
        let app = Route::new()
            .at("/data", get(get_data))
            .nest("/", StaticFilesEndpoint::new("static").index_file("index.html"))
            .with(Cors::new());
        Server::new(TcpListener::bind("0.0.0.0:3000"))
            .run(app)
            .await.unwrap()
    });

     // tokio::spawn(serve_http(parsed_data_store.clone(), recordstypes));

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
        let (parse_result, cur_parsed_line) = parser.parse(&lines);
        parsed_line_count.add_assign(u16::try_from(cur_parsed_line)?);


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
        conf.add_field(&name_s, axis, style);
    }
}
//
// async fn serve_http(result_list: Arc<Mutex<LogFields<'_>>>, conf: Arc<LogRecordsConfig>) {
//
//     // GET /hello/from/warp
//     let status = warp::path!("hello").map(
//         move || {
//             let rl = result_list.lock();
//             if rl.unwrap().is_empty() {
//                 "{}"
//             } else {
//                 "{[666]}"
//             }
//         });
//
//     let route = warp::get()
//         .and(warp::fs::dir("static/").
//             or(status)
//         );
//
//     warp::serve(route)
//         .run(([127, 0, 0, 1], 3030))
//         .await;
// }

#[derive(Clone)]
struct AppState {
    result_list: Arc<Mutex<LogFields>>,
    conf: Arc<LogRecordsConfig>
}


#[derive(Serialize)]
struct Data {
    values: Vec<u8>,
}

#[handler]
async fn get_data() -> Json<Data> {
    let mut rng = SmallRng::seed_from_u64(5711);
    let mut values = vec![0u8; 100];
    rng.fill_bytes(values.as_mut_slice());
    Json(Data { values })
}
