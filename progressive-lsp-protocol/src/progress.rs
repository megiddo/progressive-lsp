//! Standard LSP `workDoneProgress` / `$/progress`. Not a `$/` FilesSince shim.

use serde_json::{json, Value};

pub const PROGRESS_METHOD: &str = "$/progress";
pub const WORK_DONE_CREATE: &str = "window/workDoneProgress/create";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressKind {
    Begin,
    Report,
    End,
}

impl ProgressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Report => "report",
            Self::End => "end",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "begin" => Some(Self::Begin),
            "report" => Some(Self::Report),
            "end" => Some(Self::End),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkDoneProgress {
    pub token: String,
    pub kind: ProgressKind,
    pub title: Option<String>,
    pub message: Option<String>,
    pub percentage: Option<u32>,
}

impl WorkDoneProgress {
    pub fn begin(token: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            kind: ProgressKind::Begin,
            title: Some(title.into()),
            message: None,
            percentage: Some(0),
        }
    }

    pub fn report(token: impl Into<String>, message: impl Into<String>, percentage: u32) -> Self {
        Self {
            token: token.into(),
            kind: ProgressKind::Report,
            title: None,
            message: Some(message.into()),
            percentage: Some(percentage.min(100)),
        }
    }

    pub fn end(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            kind: ProgressKind::End,
            title: None,
            message: None,
            percentage: Some(100),
        }
    }

    pub fn to_notification(&self) -> Value {
        let mut value = json!({ "kind": self.kind.as_str() });
        if let Some(title) = &self.title {
            value["title"] = json!(title);
        }
        if let Some(message) = &self.message {
            value["message"] = json!(message);
        }
        if let Some(pct) = self.percentage {
            value["percentage"] = json!(pct);
        }
        json!({
            "jsonrpc": "2.0",
            "method": PROGRESS_METHOD,
            "params": {
                "token": self.token,
                "value": value
            }
        })
    }

    pub fn create_request(id: u64, token: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": WORK_DONE_CREATE,
            "params": { "token": token }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_json_is_standard_lsp() {
        assert_eq!(ProgressKind::Begin.as_str(), "begin");
        assert_eq!(ProgressKind::parse("report"), Some(ProgressKind::Report));
        assert_eq!(ProgressKind::parse("end"), Some(ProgressKind::End));
        assert_eq!(ProgressKind::parse("nope"), None);
        let n = WorkDoneProgress::begin("ingest-lib", "ingest lib").to_notification();
        assert_eq!(n["method"], PROGRESS_METHOD);
        assert_eq!(n["params"]["token"], "ingest-lib");
        assert_eq!(n["params"]["value"]["kind"], "begin");
        assert_eq!(n["params"]["value"]["title"], "ingest lib");
        let r = WorkDoneProgress::report("t", "pkg", 40).to_notification();
        assert_eq!(r["params"]["value"]["percentage"], 40);
        let e = WorkDoneProgress::end("t").to_notification();
        assert_eq!(e["params"]["value"]["kind"], "end");
        let create = WorkDoneProgress::create_request(9, "ingest-lib");
        assert_eq!(create["method"], WORK_DONE_CREATE);
        assert_eq!(create["params"]["token"], "ingest-lib");
        assert_eq!(WorkDoneProgress::report("t", "m", 200).percentage, Some(100));
    }
}
