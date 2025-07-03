use std::fs::File;
use std::io::Read;
use anyhow;

/// Read lines from a log file incrementally, while the file is being expanded by some service.
/// Keeps track of intermediate hanging "remainder" line.
/// TODO: make use of fnotify if available.
pub trait LineReader {
    fn incremental_read_line(&mut self, remainder: &mut Vec::<u8>) -> anyhow::Result<Vec<String>>;
}

impl LineReader for File {
    fn incremental_read_line(&mut self, remainder: &mut Vec::<u8>) -> anyhow::Result<Vec<String>> {
        let mut buffer = [0u8; 1024 * 10];
        let buff_n = self.read(&mut buffer)?;

        if buff_n == 0 {
            return Ok(vec![]);
        }

        remainder.extend_from_slice(&buffer[..buff_n]);
        let mut content = String::from_utf8_lossy(&remainder).into_owned();
        let lines = content.split_inclusive('\n');
        let mut result: Vec<String> = lines.map( |s| s.to_string()).collect();

        remainder.clear();
        if !content.ends_with('\n') {
            if let Some(last) = result.pop() {
                let _ = std::mem::replace(remainder, last.into_bytes());
            }
        }

        Ok(result)
    }
}
