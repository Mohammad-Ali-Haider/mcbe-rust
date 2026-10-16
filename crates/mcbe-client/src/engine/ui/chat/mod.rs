//! Session-local chat, bounded messages/history and UTF-8-safe editing.
pub(crate) mod app;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use winit::keyboard::KeyCode as K;
pub const HISTORY: usize = 64;
#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Player,
    System,
    Error,
    Command,
}
pub struct Message {
    pub text: String,
    pub kind: Kind,
    until: Instant,
}
pub struct Chat {
    scroll_limit: std::cell::Cell<usize>,
    pub open: bool,
    pub input: String,
    pub caret: usize,
    pub revision: u64,
    pub composing: bool,
    pub messages: VecDeque<Message>,
    pub scroll: usize,
    history: VecDeque<String>,
    history_index: Option<usize>,
    draft: String,
    visible: usize,
    completions: Vec<String>,
    completion: usize,
}
impl Default for Chat {
    fn default() -> Self {
        Self {
            scroll_limit: std::cell::Cell::new(0),
            open: false,
            input: String::new(),
            caret: 0,
            revision: 0,
            composing: false,
            messages: VecDeque::new(),
            scroll: 0,
            history: VecDeque::new(),
            history_index: None,
            draft: String::new(),
            visible: 0,
            completions: vec![],
            completion: 0,
        }
    }
}
impl Chat {
    pub fn begin(&mut self, slash: bool) {
        self.open = true;
        self.input = if slash { "/".into() } else { String::new() };
        self.caret = self.input.len();
        self.history_index = None;
        self.draft.clear();
        self.scroll = 0;
        self.composing = false;
        self.changed();
    }
    pub fn close(&mut self) {
        self.open = false;
        self.composing = false;
        self.scroll = 0;
        self.changed();
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.completions.clear();
        self.completion = 0;
    }
    pub fn text(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        let mut accepted = String::new();
        for c in clean.chars() {
            if self.input.len() + accepted.len() + c.len_utf8() > crate::engine::commands::MAX_INPUT
            {
                break;
            }
            accepted.push(c);
        }
        if accepted.is_empty() {
            return;
        }
        self.input.insert_str(self.caret, &accepted);
        self.caret += accepted.len();
        self.history_index = None;
        self.changed();
    }
    pub fn key(&mut self, key: K) {
        match key {
            K::ArrowLeft => {
                if self.caret > 0 {
                    self.caret = self.input[..self.caret].char_indices().last().unwrap().0;
                }
            }
            K::ArrowRight => {
                if self.caret < self.input.len() {
                    self.caret += self.input[self.caret..].chars().next().unwrap().len_utf8();
                }
            }
            K::Home => self.caret = 0,
            K::End => self.caret = self.input.len(),
            K::Backspace => {
                if self.caret > 0 {
                    let at = self.input[..self.caret].char_indices().last().unwrap().0;
                    self.input.replace_range(at..self.caret, "");
                    self.caret = at;
                }
            }
            K::Delete => {
                if self.caret < self.input.len() {
                    let end =
                        self.caret + self.input[self.caret..].chars().next().unwrap().len_utf8();
                    self.input.replace_range(self.caret..end, "");
                }
            }
            K::ArrowUp => {
                if !self.history.is_empty() {
                    let i = match self.history_index {
                        None => {
                            self.draft = self.input.clone();
                            self.history.len() - 1
                        }
                        Some(i) => i.saturating_sub(1),
                    };
                    self.history_index = Some(i);
                    self.input = self.history[i].clone();
                    self.caret = self.input.len();
                }
            }
            K::ArrowDown => {
                if let Some(i) = self.history_index {
                    if i + 1 < self.history.len() {
                        self.history_index = Some(i + 1);
                        self.input = self.history[i + 1].clone();
                    } else {
                        self.history_index = None;
                        self.input = self.draft.clone();
                    }
                    self.caret = self.input.len();
                }
            }
            _ => return,
        }
        self.changed();
    }
    pub fn submit(&mut self) -> Option<String> {
        let line = self.input.trim().to_owned();
        self.close();
        if line.is_empty() {
            return None;
        }
        if self.history.back() != Some(&line) {
            self.history.push_back(line.clone());
            if self.history.len() > HISTORY {
                self.history.pop_front();
            }
        }
        self.input.clear();
        self.caret = 0;
        Some(line)
    }
    pub fn push(&mut self, kind: Kind, text: impl Into<String>) {
        let text: String = text
            .into()
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        self.messages.push_back(Message {
            text,
            kind,
            until: Instant::now() + Duration::from_secs(10),
        });
        if self.messages.len() > HISTORY {
            self.messages.pop_front();
        }
        self.scroll = 0;
        self.changed();
    }
    pub fn tick(&mut self, now: Instant) {
        let count = self.messages.iter().filter(|m| m.until > now).count();
        if count != self.visible {
            self.visible = count;
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn lines(&self, columns: usize, rows: usize, now: Instant) -> Vec<(Kind, String)> {
        let mut lines = Vec::new();
        let columns = columns.max(1);
        for m in &self.messages {
            if !self.open && m.until <= now {
                continue;
            }
            let chars: Vec<_> = m.text.chars().collect();
            for chunk in chars.chunks(columns) {
                lines.push((m.kind, chunk.iter().collect()));
            }
        }
        self.scroll_limit.set(lines.len().saturating_sub(rows));
        let end = lines.len().saturating_sub(if self.open {
            self.scroll.min(self.scroll_limit.get())
        } else {
            0
        });
        let start = end.saturating_sub(rows);
        lines[start..end].to_vec()
    }
    pub fn scroll(&mut self, delta: i32) {
        self.scroll =
            (self.scroll as i32 + delta).clamp(0, self.scroll_limit.get() as i32) as usize;
        self.changed();
    }
    pub fn complete(
        &mut self,
        registry: &crate::engine::commands::Registry,
        client: &crate::scene::Client,
    ) {
        if self.completions.is_empty() {
            self.completions = registry.complete(client, &self.input);
            self.completion = 0;
        }
        if self.completions.is_empty() {
            return;
        }
        self.input = self.completions[self.completion % self.completions.len()].clone();
        self.caret = self.input.len();
        self.completion += 1;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn input_window(&self, columns: usize) -> String {
        let columns = columns.max(2);
        let before = self.input[..self.caret].chars().count();
        let chars: Vec<_> = self.input.chars().collect();
        let start = before.saturating_sub(columns - 1);
        let end = (start + columns - 1).min(chars.len());
        let mut result = String::new();
        for (i, ch) in chars.iter().enumerate().take(end).skip(start) {
            if i == before {
                result.push('|');
            }
            result.push(*ch);
        }
        if before == end {
            result.push('|');
        }

        result
    }
    pub fn observation(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"input":self.input,"caret":self.caret,"revision":self.revision,"history":self.history,"messages":self.messages.iter().map(|m|serde_json::json!({"kind":m.kind,"text":m.text})).collect::<Vec<_>>(),"local_session":true})
    }
}
#[cfg(test)]
mod tests;
