use std::collections::HashMap;
use regex;
use anyhow::Result;

pub struct LogRecordType {
    regex: regex::Regex,
    fields: HashMap<String, LogRecordField>
}

impl LogRecordType {
    pub fn new(regex: &str) -> anyhow::Result<LogRecordType> {
        Ok(LogRecordType {
            regex: regex::Regex::new(regex)?,
            fields: HashMap::new()
        })
    }

    pub fn parse(&mut self, lines: &Vec<String>) -> usize {
        let mut count = 0;
        for l in lines {
            let mut parsed = false;
            if let Some(cap) = self.regex.captures(&l) {
                count += 1;
                let ts: Option<f64> = cap["ts"].parse().ok();
                for (field_name, field) in self.fields.iter_mut() {
                    if field_name.as_str() != "ts" {
                        let Ok(val) = cap[field_name.as_str()].parse() else { continue };
                        field.add(ts, val);
                    }
                }
            }
        }
        count
    }

    pub fn add_field(&mut self, legend: &str, axis: Option<u8>, style: Option<&str>) {
        self.fields.insert(legend.to_string().clone(), LogRecordField::new(axis, style));
    }
}


struct LogRecordField {
    axis: Option<u8>,
    style: Option<String>,
    vals: Vec<(f64,f64)>
}

impl LogRecordField {
    fn new(axis: Option<u8>, style: Option<&str>) -> LogRecordField {
        LogRecordField {
            axis,
            style: style.and_then(|s| Some(s.to_string().clone())),
            vals: Vec::new()
        }
    }

    fn add(&mut self, ts: Option<f64>, val: f64) {
        self.vals.push((ts.unwrap_or(0f64), val));
    }
}
