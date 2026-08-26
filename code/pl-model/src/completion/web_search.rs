use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use pl_protocol::{
    WebSearchContextSize, WebSearchFilters, WebSearchUserLocation, WebSearchUserLocationType,
};

/// Web 搜索访问模式。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchMode {
    Disabled,
    #[default]
    Cached,
    Indexed,
    Live,
}

impl WebSearchMode {
    pub fn is_disabled(self) -> bool {
        self == Self::Disabled
    }
}

/// Web 搜索使用的近似位置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchLocation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl WebSearchLocation {
    pub fn is_empty(&self) -> bool {
        self.country.as_deref().is_none_or(str::is_empty)
            && self.region.as_deref().is_none_or(str::is_empty)
            && self.city.as_deref().is_none_or(str::is_empty)
            && self.timezone.as_deref().is_none_or(str::is_empty)
    }
}

/// Web 搜索的产品无关配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchConfig {
    #[serde(default)]
    pub mode: WebSearchMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_size: Option<WebSearchContextSize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<WebSearchLocation>,
}

impl WebSearchConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl From<&WebSearchLocation> for WebSearchUserLocation {
    fn from(location: &WebSearchLocation) -> Self {
        Self {
            kind: WebSearchUserLocationType::Approximate,
            country: location.country.clone(),
            region: location.region.clone(),
            city: location.city.clone(),
            timezone: location.timezone.clone(),
        }
    }
}

/// Provider-neutral Web Search 活动。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebSearchAction {
    Search {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        queries: Vec<String>,
    },
    OpenPage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    FindInPage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
    },
    #[default]
    Other,
}

/// 独立 `/alpha/search` 请求。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchRequest {
    pub id: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<serde_json::Value>>,
    pub commands: SearchCommands,
    pub settings: SearchSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

/// 独立搜索的完整命令集合。
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SearchCommands {
    /// Web search queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_query: Option<Vec<SearchQuery>>,
    /// Image search queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_query: Option<Vec<SearchQuery>>,
    /// Open pages or search result references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Vec<OpenOperation>>,
    /// Click links within an opened page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click: Option<Vec<ClickOperation>>,
    /// Find text within opened pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub find: Option<Vec<FindOperation>>,
    /// Capture PDF pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<Vec<ScreenshotOperation>>,
    /// Query finance data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finance: Option<Vec<FinanceOperation>>,
    /// Query weather data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<Vec<WeatherOperation>>,
    /// Query sports schedules or standings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sports: Option<Vec<SportsOperation>>,
    /// Query current time by UTC offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<Vec<TimeOperation>>,
    /// Desired response detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_length: Option<SearchResponseLength>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    /// Search text.
    pub q: String,
    /// Only include results from the last number of days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recency: Option<u64>,
    /// Optional domain allowlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OpenOperation {
    /// Search result reference or URL.
    pub ref_id: String,
    /// Optional line number to position the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineno: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClickOperation {
    /// Opened page reference.
    pub ref_id: String,
    /// Link id on the opened page.
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FindOperation {
    /// Opened page reference.
    pub ref_id: String,
    /// Text to find.
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotOperation {
    /// PDF page reference.
    pub ref_id: String,
    /// Zero-based PDF page number.
    pub pageno: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FinanceOperation {
    pub ticker: String,
    #[serde(rename = "type")]
    pub asset_type: FinanceAssetType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FinanceAssetType {
    Equity,
    Fund,
    Crypto,
    Index,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WeatherOperation {
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SportsOperation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<SportsToolName>,
    #[serde(rename = "fn")]
    pub function: SportsFunction,
    pub league: SportsLeague,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opponent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_games: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SportsToolName {
    Sports,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SportsFunction {
    Schedule,
    Standings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SportsLeague {
    Nba,
    Wnba,
    Nfl,
    Nhl,
    Mlb,
    Epl,
    Ncaamb,
    Ncaawb,
    Ipl,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TimeOperation {
    pub utc_offset: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchResponseLength {
    Short,
    Medium,
    Long,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_location: Option<WebSearchUserLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_context_size: Option<WebSearchContextSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<WebSearchFilters>,
    pub allowed_callers: Vec<SearchAllowedCaller>,
    pub external_web_access: ExternalWebAccess,
}

impl SearchSettings {
    pub fn from_config(config: &WebSearchConfig) -> Self {
        Self {
            user_location: config
                .location
                .as_ref()
                .filter(|location| !location.is_empty())
                .map(Into::into),
            search_context_size: config.context_size,
            filters: (!config.allowed_domains.is_empty()).then(|| WebSearchFilters {
                allowed_domains: config.allowed_domains.clone(),
            }),
            allowed_callers: vec![SearchAllowedCaller::Direct],
            external_web_access: ExternalWebAccess::from(config.mode),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchAllowedCaller {
    Direct,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ExternalWebAccess {
    Boolean(bool),
    Mode(ExternalWebAccessMode),
}

impl From<WebSearchMode> for ExternalWebAccess {
    fn from(mode: WebSearchMode) -> Self {
        match mode {
            WebSearchMode::Disabled | WebSearchMode::Cached => Self::Boolean(false),
            WebSearchMode::Indexed => Self::Mode(ExternalWebAccessMode::Indexed),
            WebSearchMode::Live => Self::Boolean(true),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalWebAccessMode {
    Indexed,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SearchResponse {
    #[serde(default)]
    pub encrypted_output: Option<String>,
    pub output: String,
    #[serde(default)]
    pub results: Option<Vec<serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_modes_map_to_external_access() {
        assert_eq!(
            [
                WebSearchMode::Disabled,
                WebSearchMode::Cached,
                WebSearchMode::Indexed,
                WebSearchMode::Live,
            ]
            .map(ExternalWebAccess::from),
            [
                ExternalWebAccess::Boolean(false),
                ExternalWebAccess::Boolean(false),
                ExternalWebAccess::Mode(ExternalWebAccessMode::Indexed),
                ExternalWebAccess::Boolean(true),
            ]
        );
    }
}
