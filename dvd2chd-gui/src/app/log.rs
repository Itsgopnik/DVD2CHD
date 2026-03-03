use super::App;

pub(super) const LOG_MAX_LINES: usize = 5_000;

#[derive(Clone)]
pub(super) struct LogEntry {
    pub(super) text: String,
    pub(super) lines: usize,
}

impl LogEntry {
    pub(super) fn new(text: String) -> Self {
        let lines = count_log_lines(&text);
        Self { text, lines }
    }
}

pub(super) fn count_log_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let newline_count = text.bytes().filter(|&b| b == b'\n').count();
    if newline_count == 0 {
        1
    } else if text.ends_with('\n') {
        newline_count
    } else {
        newline_count + 1
    }
}

impl App {
    pub(super) fn log_line(&mut self, msg: &str) {
        self.append_log_text(msg);
    }

    pub(super) fn append_log_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let entry = LogEntry::new(text.to_string());
        self.log_line_count = self.log_line_count.saturating_add(entry.lines);
        self.log.push_back(entry);
        self.trim_log_entries();
    }

    pub(super) fn trim_log_entries(&mut self) {
        while self.log_line_count > LOG_MAX_LINES {
            if let Some(entry) = self.log.pop_front() {
                self.log_line_count = self.log_line_count.saturating_sub(entry.lines);
            } else {
                break;
            }
        }
    }

    pub(super) fn clear_log(&mut self) {
        self.log.clear();
        self.log_line_count = 0;
    }

    pub(super) fn log_text(&self) -> String {
        self.log.iter().map(|entry| entry.text.as_str()).collect()
    }

    pub(super) fn log_ends_with_newline(&self) -> bool {
        self.log
            .back()
            .map(|entry| entry.text.ends_with('\n'))
            .unwrap_or(true)
    }
}
