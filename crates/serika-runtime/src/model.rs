use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A chat model row from `GET /api/chat/models`. Fields the catalog does not send stay empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "maxTokens", deserialize_with = "opt_u64")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub premium: bool,
    #[serde(default, rename = "isDefault")]
    pub is_default: bool,
    #[serde(default, rename = "usageMultiplier", deserialize_with = "opt_f64")]
    pub usage_multiplier: Option<f64>,
    #[serde(default)]
    pub availability: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ModelCatalog {
    #[serde(default)]
    pub free: Vec<ChatModel>,
    #[serde(default)]
    pub premium: Vec<ChatModel>,
}

impl ModelCatalog {
    pub fn all(&self) -> impl Iterator<Item = &ChatModel> {
        self.premium.iter().chain(self.free.iter())
    }

    pub fn find(&self, id: &str) -> Option<&ChatModel> {
        self.all().find(|model| model.id == id)
    }

    /// Prefer a coding-oriented free model that is actually in this catalog.
    /// The server's `isDefault` free model is not necessarily a coding model.
    pub fn default_coding_model(&self) -> Option<String> {
        const PREFERRED: &[&str] = &[
            "deepseek/deepseek-v4.1-flash",
            "mistralai/mistral-small-2603",
            "google/gemma-4-31b-it",
            "google/gemma-4-26b-a4b-it",
        ];
        for id in PREFERRED {
            if self.free.iter().any(|model| model.id == *id) {
                return Some((*id).to_string());
            }
        }
        self.free
            .iter()
            .find(|model| model.is_default)
            .or_else(|| self.free.first())
            .map(|model| model.id.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageWindow {
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub used: Option<u64>,
    #[serde(default)]
    pub remaining: Option<u64>,
    #[serde(default)]
    pub percent: Option<f64>,
    #[serde(default, rename = "startedAt")]
    pub started_at: Option<String>,
    #[serde(default, rename = "resetsAt")]
    pub resets_at: Option<String>,
    #[serde(default, rename = "windowHours")]
    pub window_hours: Option<u64>,
    #[serde(default, rename = "weekStart")]
    pub week_start: Option<String>,
    #[serde(default, rename = "byKind")]
    pub by_kind: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyWindow {
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub used: Option<u64>,
    #[serde(default)]
    pub remaining: Option<u64>,
    #[serde(default, rename = "resetsAt")]
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageTier {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsageFeatures {
    #[serde(default)]
    pub video: Option<bool>,
    #[serde(default, rename = "webSearch")]
    pub web_search: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResetOffer {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub cents: Option<u64>,
}

/// `plan` object from `GET /api/chat/usage`. Absent sections are not invented.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsagePlan {
    #[serde(default)]
    pub tier: Option<UsageTier>,
    #[serde(default, rename = "isPremium")]
    pub is_premium: Option<bool>,
    #[serde(default)]
    pub byok: Option<bool>,
    #[serde(default)]
    pub features: Option<UsageFeatures>,
    #[serde(default)]
    pub session: Option<UsageWindow>,
    #[serde(default)]
    pub weekly: Option<UsageWindow>,
    #[serde(default)]
    pub daily: Option<DailyWindow>,
    #[serde(default, rename = "lastReset")]
    pub last_reset: Option<Value>,
    #[serde(default, rename = "resetOffer")]
    pub reset_offer: Option<ResetOffer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsageSnapshot {
    #[serde(default, rename = "totalMessages")]
    pub total_messages: Option<u64>,
    #[serde(default, rename = "dailyMessages")]
    pub daily_messages: Option<u64>,
    #[serde(default, rename = "lastReset")]
    pub last_reset: Option<Value>,
    #[serde(default)]
    pub plan: Option<UsagePlan>,
}

impl UsageSnapshot {
    pub fn summary_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(plan) = &self.plan {
            if let Some(tier) = &plan.tier {
                if let Some(name) = &tier.name {
                    parts.push(name.clone());
                }
            }
            if let Some(week) = &plan.weekly {
                if let (Some(used), Some(limit)) = (week.used, week.limit) {
                    let remaining = week.remaining.unwrap_or(limit.saturating_sub(used));
                    parts.push(format!("weekly {used}/{limit} ({remaining} left)"));
                }
                if let Some(resets) = &week.resets_at {
                    parts.push(format!("resets {resets}"));
                }
            }
            if let Some(session) = &plan.session {
                if let (Some(used), Some(limit)) = (session.used, session.limit) {
                    parts.push(format!("session {used}/{limit}"));
                }
            }
            if let Some(daily) = &plan.daily {
                if let (Some(used), Some(limit)) = (daily.used, daily.limit) {
                    parts.push(format!("daily messages {used}/{limit}"));
                }
            }
        }
        if parts.is_empty() {
            if let Some(total) = self.total_messages {
                return format!("{total} messages recorded");
            }
            return "No usage figures in the server response.".to_string();
        }
        parts.join(" · ")
    }
}

/// Body of a 429 from `usageMeter.UsageLimitError.toJSON()`, plus any extra fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsageLimitBody {
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default, rename = "limitReached")]
    pub limit_reached: Option<bool>,
    #[serde(default, rename = "upgradeUrl")]
    pub upgrade_url: Option<String>,
    #[serde(default, rename = "byokAvailable")]
    pub byok_available: Option<bool>,
    #[serde(default, rename = "resetUrl")]
    pub reset_url: Option<String>,
    #[serde(default, rename = "resetPrice")]
    pub reset_price: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub used: Option<u64>,
    #[serde(default)]
    pub cost: Option<u64>,
    #[serde(default, rename = "resetsAt")]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub feature: Option<String>,
}

impl std::fmt::Display for UsageLimitBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = self
            .error
            .clone()
            .unwrap_or_else(|| "The Serika usage limit was reached.".to_string());
        write!(f, "{message}")?;
        if let Some(code) = &self.code {
            write!(f, " [{code}]")?;
        }
        if let (Some(used), Some(limit)) = (self.used, self.limit) {
            write!(f, " Used {used} of {limit}.")?;
        }
        if let Some(resets) = &self.resets_at {
            write!(f, " Resets {resets}.")?;
        }
        if let Some(url) = &self.reset_url {
            write!(f, " Reset: {url}")?;
            if let Some(price) = &self.reset_price {
                write!(f, " ({price})")?;
            }
            write!(f, ".")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountProfile {
    pub id: String,
    pub username: String,
    pub email: String,
    pub is_premium: bool,
    pub is_verified: Option<bool>,
    pub subscription_status: Option<String>,
    pub subscription_plan: Option<String>,
    pub subscription_period_end: Option<String>,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
}

impl AccountProfile {
    pub fn from_value(value: &Value) -> Option<Self> {
        let obj = user_object(value, 0)?;
        let id = first_present([obj.get("id"), obj.get("_id"), obj.get("userId")]);
        let username = json_string(obj.get("username"));
        if id.is_empty() && username.is_empty() {
            return None;
        }
        Some(Self {
            id,
            username,
            email: json_string(obj.get("email")),
            is_premium: obj
                .get("isPremium")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            is_verified: obj.get("isVerified").and_then(|v| v.as_bool()),
            subscription_status: optional_string(obj.get("subscriptionStatus")),
            subscription_plan: optional_string(obj.get("subscriptionPlan")),
            subscription_period_end: optional_string(obj.get("subscriptionPeriodEnd")),
            display_name: optional_string(obj.get("displayName")),
            avatar: avatar_url(obj),
        })
    }
}

fn user_object(value: &Value, depth: u8) -> Option<&serde_json::Map<String, Value>> {
    let obj = value.as_object()?;
    let looks_like_user = obj.contains_key("username")
        || obj.contains_key("avatar")
        || obj.contains_key("avatarUrl")
        || obj.contains_key("isPremium")
        || obj.contains_key("email");
    if looks_like_user {
        return Some(obj);
    }
    if depth >= 3 {
        return None;
    }
    for key in ["user", "account", "data", "profile"] {
        if let Some(inner) = obj.get(key) {
            if let Some(found) = user_object(inner, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

fn first_present<'a>(values: [Option<&'a Value>; 3]) -> String {
    values
        .into_iter()
        .map(json_string)
        .find(|text| !text.is_empty())
        .unwrap_or_default()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    let text = json_string(value);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn avatar_url(obj: &serde_json::Map<String, Value>) -> Option<String> {
    for key in ["avatar", "avatarUrl", "profilePicture", "image", "photo"] {
        match obj.get(key) {
            Some(Value::String(text)) if !text.trim().is_empty() => {
                return Some(text.trim().to_string());
            }
            Some(Value::Object(inner)) => {
                if let Some(url) = optional_string(inner.get("url")) {
                    return Some(url);
                }
            }
            _ => {}
        }
    }
    None
}

/// Turn a profile avatar into an https URL. Relative paths are served from serika.dev.
pub fn absolute_avatar_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.starts_with("https://") {
        return Some(raw.to_string());
    }
    if let Some(rest) = raw.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if let Some(path) = raw.strip_prefix('/') {
        return Some(format!("https://serika.dev/{path}"));
    }
    None
}

fn json_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn opt_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    })
}

fn opt_f64<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_and_picks_a_live_catalog_model() {
        let catalog: ModelCatalog = serde_json::from_str(
            r#"{"free":[{"id":"sao10k/l3-lunaris-8b","name":"Lunaris","isDefault":true,"maxTokens":"4096"},{"id":"deepseek/deepseek-v4.1-flash","name":"DeepSeek"}],"premium":[{"id":"x-ai/grok-build-0.1","name":"Grok Build","premium":true,"usageMultiplier":4.2}]}"#,
        )
        .unwrap();
        assert_eq!(catalog.free[0].max_tokens, Some(4096));
        assert_eq!(
            catalog.default_coding_model().as_deref(),
            Some("deepseek/deepseek-v4.1-flash")
        );
        assert!(catalog.find("x-ai/grok-build-0.1").unwrap().premium);

        let nested = AccountProfile::from_value(&serde_json::json!({
            "user": {
                "id": 42,
                "username": "pikachubolk",
                "email": "ada@example.com",
                "isPremium": true,
                "avatar": "https://cdn.serika.dev/avatars/ada.png",
                "subscriptionPlan": "AI Max",
                "subscriptionPeriodEnd": "2026-12-01T00:00:00.000Z"
            }
        }))
        .unwrap();
        assert_eq!(nested.username, "pikachubolk");
        assert_eq!(nested.id, "42");
        assert_eq!(
            nested.avatar.as_deref(),
            Some("https://cdn.serika.dev/avatars/ada.png")
        );
        assert_eq!(nested.subscription_plan.as_deref(), Some("AI Max"));
        assert!(nested.subscription_period_end.is_some());
        assert_eq!(
            absolute_avatar_url("/uploads/ada.png").as_deref(),
            Some("https://serika.dev/uploads/ada.png")
        );
        assert!(AccountProfile::from_value(&serde_json::json!({"ok": true})).is_none());

        let usage: UsageSnapshot = serde_json::from_str(
            r#"{"totalMessages":3,"dailyMessages":1,"lastReset":null,"plan":{"tier":{"key":"free","name":"Free"},"isPremium":false,"weekly":{"limit":10000,"used":20,"remaining":9980,"resetsAt":"2026-11-06T00:00:00.000Z"},"session":{"limit":5000,"used":20,"remaining":4980,"windowHours":5}}}"#,
        )
        .unwrap();
        let line = usage.summary_line();
        assert!(line.contains("Free"));
        assert!(line.contains("9980"));
        assert!(line.contains("2026-11-06"));

        let limit: UsageLimitBody = serde_json::from_str(
            r#"{"error":"You've used this week's free allowance.","code":"WEEKLY_LIMIT_REACHED","limitReached":true,"upgradeUrl":"/premium","tier":"free","limit":10000,"used":10000,"resetsAt":"2026-11-06T00:00:00.000Z"}"#,
        )
        .unwrap();
        let text = limit.to_string();
        assert!(text.contains("WEEKLY_LIMIT_REACHED"));
        assert!(text.contains("10000"));
    }
}
