use std::collections::HashMap;
use regex;
use std::sync::Arc;
use std::collections::LinkedList;


pub type LogRecordsConfig = HashMap<String, LogRecordType>;
pub type FieldSample = (f64, f64);
// pub type ParsedBlock<'a> = HashMap<&'a String, Vec<FieldSample>>;
pub type LogFields<'a> = LinkedList<ParsedBlock<'a>>;

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
            fields: HashMap::new()
        })
    }

    pub fn add_field(&mut self, legend: &str, axis: Option<u8>, style: Option<&str>) {
        let field_name = self.name.clone() + legend;
        self.fields.insert(legend.to_string().clone(), LogRecordField::new(field_name, axis, style));
    }
}


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
}

impl LogParser {
    pub fn new(record_type: Arc<LogRecordsConfig>) -> LogParser {
        LogParser { records_conf: record_type }
    }

    pub fn parse(&self, lines: &Vec<String>) -> (ParsedBlock, usize) {
        let mut count = 0;
        let mut result = ParsedBlock::new();
        let mut res_ts = Option::<f64>::None;
        for rec in self.records_conf.values() {
            for field in rec.fields.values() {
                let field_name = &field.name;
                result.get_map_mut().insert(field_name, Vec::<FieldSample>::new());
            }
        }

        for l in lines {
            let mut parsed = false;
            for rec in self.records_conf.values() {
                if let Some(cap) = rec.regex.captures(&l) {
                    count += 1;
                    let ts: Option<f64> = cap["ts"].parse().ok();
                    for (field_name, field) in rec.fields.iter() {
                        let field_name = &field.name;
                        let Some(ref mut vec)
                            = result.get_map_mut().get_mut(field_name) else { continue };

                        if field_name.as_str() != "ts" {
                            let Ok(val) = cap[field_name.as_str()].parse() else { continue };
                            let ts_to_push = ts.unwrap_or(0f64);
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
        }
        (result, count)
    }
}

struct ParsedBlock<'a> {
    data: HashMap<&'a String, Vec<FieldSample>>,
    ts: Option<f64>,
}

impl<'a> ParsedBlock<'a> {
    pub fn new() -> ParsedBlock<'a> {
        ParsedBlock {data: HashMap::new(), ts: None}
    }

    pub fn set_ts(&mut self, ts: f64) {
        self.ts = Some(ts);
    }

    pub fn get_ts(&self) -> Option<f64> {
        self.ts
    }

    pub fn get_map_mut(&mut self) -> &mut HashMap<&'a String, Vec<FieldSample>> {
        &mut self.data
    }

    pub fn get_map(&self) -> &HashMap<&'a String, Vec<FieldSample>> {
        &self.data
    }
}

pub trait Append {
    fn append_result(&mut self, result: ParsedBlock, max_duration: Option<f64>);
}

impl Append for LogFields<'_> {
    fn append_result(&mut self, result: ParsedBlock, max_duration: Option<f64>) {
        let new_ts = result.get_ts();
        if max_duration.is_some() && new_ts.is_some() {
            while !self.is_empty() {
                let oldest = self.back().unwrap();
                let Some(mut oldest_ts) = oldest.get_ts() else { break; };
                let ts_delta = (new_ts.unwrap() - oldest_ts).abs();
                if ts_delta > max_duration.unwrap() {
                    self.pop_back();
                } else {
                    break;
                }
            }
        }
        self.push_front(result);
    }
}
