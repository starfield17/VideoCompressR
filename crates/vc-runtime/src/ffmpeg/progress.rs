use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressUpdate {
    pub values: BTreeMap<String, String>,
    pub is_end: bool,
}

#[derive(Default)]
pub struct ProgressParser {
    buffer: String,
    values: BTreeMap<String, String>,
}

impl ProgressParser {
    pub fn push(&mut self, chunk: &str) -> Vec<ProgressUpdate> {
        self.buffer.push_str(chunk);
        let mut updates = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            let line = self.buffer[..index].trim_end_matches('\r').to_owned();
            self.buffer.drain(..=index);
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_owned();
                let value = value.trim().to_owned();
                if key == "progress" {
                    self.values.insert(key, value.clone());
                    updates.push(ProgressUpdate {
                        values: std::mem::take(&mut self.values),
                        is_end: value == "end",
                    });
                } else if !key.is_empty() {
                    self.values.insert(key, value);
                }
            }
        }
        updates
    }
    pub fn finish(&mut self) -> Option<ProgressUpdate> {
        if self.buffer.is_empty() && self.values.is_empty() {
            return None;
        }
        let tail = std::mem::take(&mut self.buffer);
        let updates = self.push(&(tail + "\n"));
        updates.into_iter().last()
    }
}

pub fn progress_percent(update: &ProgressUpdate, duration_sec: Option<f64>) -> Option<f64> {
    let raw = update.values.get("out_time_us")?.parse::<f64>().ok()? / 1_000_000.0;
    let duration = duration_sec.filter(|value| *value > 0.0)?;
    Some((raw / duration * 100.0).clamp(0.0, 100.0))
}
