use std::collections::HashMap;
use regex;
use std::sync::Arc;
use serde::Serialize;
use serde_json;

pub type LogRecordsConfig = HashMap<String, LogRecordType>;
pub type FieldSample = (f64, f64);


#[derive(Serialize)]
struct LogRecordTypeJson {
    name: String,
    fields: HashMap<String, LogRecordField>
}

// Implement a method to convert LogRecordsConfig to JSON
pub trait ToJson {
    fn to_json(&self) -> anyhow::Result<String>;
}

impl ToJson for LogRecordsConfig {
    fn to_json(&self) -> anyhow::Result<String> {
        let mut json_map = HashMap::new();

        for (key, record_type) in self.iter() {
            // Create a version of LogRecordType without the regex
            let json_record = LogRecordTypeJson {
                name: record_type.name.clone(),
                fields: record_type.fields.clone(),
            };

            json_map.insert(key.clone(), json_record);
        }

        // Serialize the map to JSON
        let json_string = serde_json::to_string_pretty(&json_map)?;
        Ok(json_string)
    }
}

// Example usage:
// let json = my_log_records_config.to_json()?;


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

    pub fn add_field(&mut self, legend: &str, axis: Option<u8>, style: Option<&str>) {
        let field_name = legend.to_string();
        self.fields.insert(legend.to_string().clone(), LogRecordField::new(field_name, axis, style));
    }
}


#[derive(Serialize, Clone)]
struct LogRecordField {
    name: String,
    axis: Option<u8>,
    style: Option<String>,
}

impl LogRecordField {
    fn new(name: String, axis: Option<u8>, style: Option<&str>) -> LogRecordField {
        LogRecordField {
            name: name,
            axis,
            style: style.and_then(|s| Some(s.to_string().clone()))
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

    pub fn parse(&mut self, lines: &Vec<String>) -> (ParsedBlock, usize) {
        let mut count = 0;
        let mut result = ParsedBlock::new();
        let mut res_ts = Option::<f64>::None;
        for rec in self.records_conf.values() {
            for field in rec.fields.values() {
                let field_name = &field.name;
                result.get_map_mut().insert(field_name.clone(), Vec::<FieldSample>::new());
            }
        }

        for l in lines {
            let mut parsed = false;
            for rec in self.records_conf.values() {
                if let Some(cap) = rec.regex.captures(&l) {
                    count += 1;
                    let ts: Option<f64> = cap["ts"].parse().ok();
                    if self.ts_init.is_none() { self.ts_init = ts; }
                    for (field_name, field) in rec.fields.iter() {
                        let field_name = &field.name;
                        let Some(ref mut vec)
                            = result.get_map_mut().get_mut(field_name) else { continue };

                        if field_name.as_str() != "ts" {
                            let Ok(val) = cap[field_name.as_str()].parse() else { continue };
                            let ts_to_push = (ts.unwrap_or(0f64)
                                                   - self.ts_init.unwrap_or(0f64)) * 1e-9;
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
                            vec.push((ts_to_push, val));
                        }
                    }
                }
            }
        }
        if res_ts.is_some() {
            result.set_ts(res_ts.unwrap());
        } else {
            result.set_ts(self.results_counter as f64);
        }
        self.results_counter += 1;
        (result, count)
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

    pub fn get_map(&self) -> &HashMap<String, Vec<FieldSample>> {
        &self.data
    }
}
