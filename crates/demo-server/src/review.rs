//! Fixed synthetic review content. Choices record a boolean; they never dispatch tools.
use serde_json::{Value, json};

pub const AGENT_ID: &str = "simulated-review";
pub const PROMPT: &str = "Simulated coding-agent review: no AI, tools or host commands run.\nSimulated tool request: display a pretend test summary (no tests run).\nIn this virtual terminal type review allow or review deny.\nOne choice per review terminal; delete it and reconnect to try again.\n";

#[derive(Default)]
pub struct Review {
    pub decision: Option<bool>,
}
impl Review {
    pub fn command(&mut self, args: &[String]) -> Result<String, String> {
        match args {
            [] => Ok(match self.decision {
                None => PROMPT.into(),
                Some(allow) => format!(
                    "Simulated choice: {}; nothing was executed.\n",
                    if allow { "allow" } else { "deny" }
                ),
            }),
            [choice] if choice == "allow" || choice == "deny" => {
                if self.decision.is_some() {
                    return Err("Simulated request already answered; nothing was executed".into());
                }
                self.decision = Some(choice == "allow");
                Ok(format!(
                    "Simulated choice recorded: {choice}; nothing was executed.\n"
                ))
            }
            _ => Err("Simulated review accepts only review, review allow or review deny".into()),
        }
    }

    pub fn detail(&self) -> &'static str {
        if self.decision.is_some() {
            "Simulated choice recorded; no AI or tools executed."
        } else {
            "Simulated prompt: type review allow or review deny in the virtual terminal. No AI or tools execute."
        }
    }

    pub fn messages(&self, after: Option<&str>, before: Option<&str>) -> Value {
        let mut messages = vec![
            json!({"id":"simulated-notice","role":"system","ts":null,"blocks":[{"kind":"text","text":PROMPT}]}),
            json!({"id":"simulated-user","role":"user","ts":null,"blocks":[{"kind":"text","text":"Simulated user request: show an example coding-agent companion workflow."}]}),
            json!({"id":"simulated-tool","role":"assistant","ts":null,"blocks":[{"kind":"tool-use","name":"Simulated summary","input":"Simulated request only: pretend test summary. Type review allow or review deny in the virtual terminal; no tools execute."}]}),
        ];
        if let Some(allow) = self.decision {
            messages.push(json!({"id":"simulated-result","role":"assistant","ts":null,"blocks":[{"kind":"tool-result","ok":allow,"text":format!("Simulated terminal choice: {}; nothing was executed. This is a fixed demonstration, not a live agent response.", if allow { "allow" } else { "deny" })}]}));
        }
        // Two fixed revisions only. Unknown cursors cause a complete, bounded reset.
        let cursor = if self.decision.is_some() {
            "simulated-answered"
        } else {
            "simulated-waiting"
        };
        let reset = match after {
            Some(c) if c == cursor => {
                messages.clear();
                false
            }
            Some("simulated-waiting") if self.decision.is_some() => {
                messages.drain(..3);
                false
            }
            _ => true,
        };
        if before.is_some() {
            messages.clear();
        }
        json!({"source":"transcript","messages":messages,"cursor":cursor,"reset":reset && before.is_none(),"more":false,"older_cursor":null})
    }
}

pub fn agents() -> Value {
    json!({"agents":[{"id":AGENT_ID,"name":"Simulated coding-agent review","detected":{"installed":false,"confidence":"high","reason":"simulated","detail":"Virtual fixture only. No coding agent is installed or executed.","config_path":null},"integration":"not-installed","activation":null,"conflicts":[]}]})
}
