//! Registered local commands: parse arguments once and reuse engine transactions.
mod give;
mod spawn;
mod travel;
use crate::scene::{Client, Result};
use std::{collections::BTreeMap, ops::RangeInclusive};
pub const MAX_INPUT: usize = 512;
#[derive(Clone, Debug)]
pub struct Invocation {
    pub name: String,
    pub args: Vec<String>,
}
impl Invocation {
    pub fn validate(&self) -> Result<()> {
        if !crate::engine::blocks::state::identifier(&self.name)
            || self.args.len() > 32
            || self.name.len() + self.args.iter().map(|s| s.len() + 1).sum::<usize>() > MAX_INPUT
            || self.args.iter().any(|s| s.chars().any(char::is_control))
        {
            return Err("Invalid game-command invocation".into());
        }
        Ok(())
    }
}
pub struct Outcome {
    pub message: String,
    pub travel: Option<String>,
}
impl Outcome {
    pub fn message(message: String) -> Self {
        Self {
            message,
            travel: None,
        }
    }
}
pub struct Command {
    pub usage: &'static str,
    pub arguments: RangeInclusive<usize>,
    pub run: fn(&mut Client, &[String]) -> Result<Outcome>,
    pub complete: fn(&Client) -> Vec<String>,
}
pub struct Registry {
    entries: BTreeMap<String, Command>,
}
impl Registry {
    pub fn built_in() -> Self {
        let mut r = Self {
            entries: BTreeMap::new(),
        };
        r.register(
            "give",
            Command {
                usage: "/give <item> [count]",
                arguments: 1..=2,
                run: give::run,
                complete: give::complete,
            },
        )
        .expect("Give command");
        r.register(
            "travel",
            Command {
                usage: "/travel <dimension>",
                arguments: 1..=1,
                run: travel::run,
                complete: travel::complete,
            },
        )
        .expect("Travel command");
        r.register(
            "spawn",
            Command {
                usage: "/spawn <entity>",
                arguments: 1..=1,
                run: spawn::run,
                complete: spawn::complete,
            },
        )
        .expect("Spawn command");
        r
    }
    pub fn register(&mut self, name: &str, command: Command) -> Result<()> {
        if !crate::engine::blocks::state::identifier(name)
            || self.entries.contains_key(name)
            || command.arguments.is_empty()
            || *command.arguments.end() > 32
        {
            return Err("Invalid or duplicate command registration".into());
        }
        self.entries.insert(name.into(), command);
        Ok(())
    }
    pub fn execute(&self, client: &mut Client, line: &str) -> Result<Outcome> {
        if line.len() > MAX_INPUT {
            return Err("Command exceeds 512 bytes".into());
        }
        let line = line.strip_prefix('/').ok_or("Commands start with /")?;
        let words = arguments(line)?;
        let name = words
            .first()
            .ok_or("Enter a command; use Tab to see registered names")?;
        self.dispatch(
            client,
            &Invocation {
                name: name.clone(),
                args: words[1..].to_vec(),
            },
        )
    }
    pub fn dispatch(&self, client: &mut Client, invocation: &Invocation) -> Result<Outcome> {
        invocation.validate()?;
        let name = &invocation.name;
        let command = self.entries.get(name).ok_or_else(|| {
            format!(
                "Unknown command /{name}. Available: {}",
                self.entries
                    .keys()
                    .map(|k| format!("/{k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        let args = &invocation.args;
        if !command.arguments.contains(&args.len()) {
            return Err(format!("Usage: {}", command.usage).into());
        }
        (command.run)(client, args)
    }
    pub fn complete(&self, client: &Client, input: &str) -> Vec<String> {
        let Some(line) = input.strip_prefix('/') else {
            return vec![];
        };
        let words: Vec<_> = line.split_whitespace().collect();
        if words.len() <= 1 && !line.ends_with(char::is_whitespace) {
            return self
                .entries
                .keys()
                .filter(|k| k.starts_with(line))
                .map(|k| format!("/{k} "))
                .collect();
        }
        let Some(name) = words.first() else {
            return vec![];
        };
        let Some(command) = self.entries.get(*name) else {
            return vec![];
        };
        if words.len() > 2 || (words.len() == 2 && line.ends_with(char::is_whitespace)) {
            return vec![];
        }
        let prefix = if line.ends_with(char::is_whitespace) {
            ""
        } else {
            words.get(1).copied().unwrap_or("")
        };
        let mut values = (command.complete)(client);
        values.sort();
        values.dedup();
        values
            .into_iter()
            .filter(|v| v.starts_with(prefix))
            .map(|v| format!("/{name} {v} "))
            .filter(|v| v.len() <= MAX_INPUT && !v.chars().any(char::is_control))
            .take(256)
            .collect()
    }
}
/// Whitespace, quoted arguments and escaped quotes/backslashes; no shell evaluation.
pub fn arguments(line: &str) -> Result<Vec<String>> {
    if line.len() > MAX_INPUT || line.chars().any(char::is_control) {
        return Err("Command exceeds 512 bytes or contains control characters".into());
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    let mut started = false;
    for ch in line.chars() {
        if escape {
            word.push(ch);
            escape = false;
            started = true;
            continue;
        }
        if ch == '\\' {
            escape = true;
            started = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            started = true;
        } else if ch.is_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(ch);
            started = true;
        }
        if words.len() > 33 {
            return Err("Too many command arguments".into());
        }
    }
    if quote.is_some() || escape {
        return Err("Unclosed quote or trailing escape in command".into());
    }
    if started {
        words.push(word);
    }
    if words.len() > 33 {
        return Err("Too many command arguments".into());
    }
    Ok(words)
}
#[cfg(test)]
mod tests;
