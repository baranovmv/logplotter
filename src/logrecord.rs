use std::collections::HashMap;
use regex;
use std::sync::Arc;
use serde::Serialize;
use serde_json;

pub type FieldSample = (f64, f64);

pub struct LogRecordsConfig {
    pub(crate) records: HashMap<String, LogRecordType>,
    pub title: String
}

impl LogRecordsConfig {
    pub fn new(title: String) -> LogRecordsConfig {
        let records = HashMap::new();
        LogRecordsConfig{title, records}
    }
}

#[derive(Serialize)]
struct LogRecordTypeJson {
    name: String,
    fields: HashMap<String, LogRecordField>
}

// Converts LogRecordsConfig to JSON
pub trait ToJson {
    fn to_json(&self) -> anyhow::Result<String>;
}

impl ToJson for LogRecordsConfig {
    fn to_json(&self) -> anyhow::Result<String> {
        let mut json_map = HashMap::new();

        for (key, record_type) in self.records.iter() {
            // Creating a version of LogRecordType without the regex
            let json_record = LogRecordTypeJson {
                name: record_type.name.clone(),
                fields: record_type.fields.clone(),
            };

            json_map.insert(key.clone(), json_record);
        }
        // json_map.insert(String::from("title"), self.title.clone());
        let json = serde_json::json!({
            "title": self.title,
            "fields": json_map,
        });

        // Serializing the map to JSON
        // let json_string = serde_json::to_string_pretty(&json_map)?;
        Ok(json.to_string())
    }
}


pub struct LogRecordType {
    name: String,
    regex: regex::Regex,
    fields: HashMap<String, LogRecordField>
}

impl LogRecordType {
    pub fn new(name: &str, regex: &str) -> anyhow::Result<LogRecordType> {
        Ok(LogRecordType {
            name: name.to_string().clone(),
            regex: regex::Regex::new(regex)?,
            fields: HashMap::new(),
        })
    }

    pub fn add_field(&mut self, legend: &str, axis: Option<u8>, style: Option<&str>,
                     coef: Option<f64>, ylim: Option::<Vec<f64>>) {
        let field_name = legend.to_string();
        self.fields.insert(legend.to_string().clone(), LogRecordField::new(field_name, axis, style,
                                                                           coef, ylim));
    }
}


#[derive(Serialize, Clone)]
struct LogRecordField {
    name: String,
    axis: Option<u8>,
    style: Option<String>,
    coef: Option<f64>,
    ylim: Option<Vec<f64>>
}

impl LogRecordField {
    fn new(name: String, axis: Option<u8>, style: Option<&str>, coef: Option<f64>, ylim: Option<Vec<f64>>) -> LogRecordField {
        LogRecordField {
            name,
            axis,
            style: style.and_then(|s| Some(s.to_string().clone())),
            coef,
            ylim,
        }
    }
}

pub struct LogParser {
    records_conf: Arc<LogRecordsConfig>,
    results_counter: usize,
    ts_init: Option<f64>,
}

impl LogParser {
    pub fn new(record_type: Arc<LogRecordsConfig>) -> LogParser {
        LogParser { records_conf: record_type, results_counter: 0, ts_init: None }
    }

    pub fn parse(&mut self, lines: &Vec<String>) -> Option<(ParsedBlock, usize)> {
        let mut count = 0;
        let mut result = ParsedBlock::new();
        let mut res_ts = Option::<f64>::None;
        let mut parsed = false;

        for l in lines {
            for rec in self.records_conf.records.values() {
                if let Some(cap) = rec.regex.captures(&l) {
                    parsed = true;
                    count += 1;
                    // Add fields to the result if there is none.
                    for field in rec.fields.values() {
                        let field_name = &field.name;
                        let ref mut res_map = result.get_map_mut();
                        if !res_map.contains_key(field_name) {
                            res_map.insert(field_name.clone(), Vec::<FieldSample>::new());
                        }
                    }
                    let ts: Option<f64> = if cap.name("ts").is_some() {
                        cap.name("ts").unwrap().as_str().parse().ok()
                    } else if cap.name("time_ts").is_some() {
                        self.parse_time(&cap["time_ts"]).ok()
                    } else {
                        None
                    };
                    if self.ts_init.is_none() { self.ts_init = ts; }
                    for (_field_name, field) in rec.fields.iter() {
                        let field_name = &field.name;
                        let Some(ref mut vec)
                            = result.get_map_mut().get_mut(field_name) else { continue };

                        if field_name.as_str() != "ts" || field_name.as_str() != "time_ts" {
                            let val_str = &cap[field_name.as_str()];
                            let mut val: f64;
                            if val_str.parse::<f64>().is_ok() {
                                val = val_str.parse::<f64>().unwrap();
                            } else {
                                if let Ok(val_time) = self.parse_time(val_str) {
                                    val = val_time;
                                }  else {
                                    continue;
                                };
                            }
                            let ts_to_push = ts.unwrap_or(0f64)
                                                   - self.ts_init.unwrap_or(0f64);
                            if ts.is_none() {
                                eprintln!("Error, ts is none for {}", field_name);
                            }
                            if ts.is_some() {
                                if res_ts.is_some() {
                                    res_ts = Some(res_ts.unwrap().max(ts_to_push));
                                } else {
                                    res_ts = Some(ts_to_push);
                                }
                            }
                            val = val * field.coef.unwrap_or(1f64);
                            if let Some(ref ylim) = field.ylim {
                                if ylim.len() == 2 {
                                    val = val.clamp(ylim[0], ylim[1]);
                                }
                            }
                            vec.push((ts_to_push, val * field.coef.unwrap_or(1f64)));
                        }
                    }
                }
            }
        }
        if !parsed {
            return None
        }
        if res_ts.is_some() {
            result.set_ts(res_ts.unwrap());
        } else {
            result.set_ts(self.results_counter as f64);
        }
        self.results_counter += 1;
        Some((result, count))
    }

    // Convert string of format [+-]hr:mn:sec.0123456789 into sec related to 0.
    fn parse_time(&self, s: &str) -> Result::<f64, ()> {
        let negative = s.starts_with('-');
        let trimmed = s.trim_start_matches(|c| c == '-' || c == '+');

        let parts: Vec<&str> = trimmed.split(':').collect();
        if parts.len() != 3 {
            return Err(());
        }

        let hours: i32 = parts[0].parse().map_err(|_| ())?;
        let minutes: i32 = parts[1].parse().map_err(|_| ())?;

        let seconds: f64 = parts[2].parse().map_err(|_| ())?;

        let total_sec: f64 = f64::from(hours) * 3600f64 + f64::from(minutes) * 60f64 + seconds;

        Ok(if negative { -total_sec } else { total_sec })
    }
}

#[derive(Serialize, Clone)]
pub struct ParsedBlock {
    data: HashMap<String, Vec<FieldSample>>,
    ts: f64
}

impl ParsedBlock {
    pub fn new() -> ParsedBlock {
        ParsedBlock {data: HashMap::new(), ts: 0f64}
    }

    pub fn set_ts(&mut self, ts: f64) {
        self.ts = ts;
    }

    pub fn get_ts(&self) -> f64 {
        self.ts
    }

    pub fn get_map_mut(&mut self) -> &mut HashMap<String, Vec<FieldSample>> {
        &mut self.data
    }
}
