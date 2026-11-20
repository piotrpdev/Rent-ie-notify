//! Run configuration, read from environment variables and validated up front.

use anyhow::{Context as _, Result, bail};

use crate::places::{self, Place};

/// Search sources; at least one is set.
#[derive(Debug, Default)]
pub(crate) struct Sources {
    pub(crate) rent_ie_url: Option<String>,
    /// JSON request body for the daft.ie gateway.
    pub(crate) daft_ie_request: Option<String>,
    pub(crate) property_ie_url: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Config {
    pub(crate) sources: Sources,
    /// Required unless `dry_run` or `seed_only`.
    pub(crate) webhook_url: Option<String>,
    pub(crate) ors_api_key: Option<String>,
    pub(crate) text_report_dir: Option<String>,
    pub(crate) text_report_include_all: bool,
    pub(crate) max_beds: Option<u32>,
    pub(crate) places: Vec<Place>,
    /// `DRY_RUN`: print Slack payloads and never write state.
    pub(crate) dry_run: bool,
    /// `SEED_SENT_IDS`: record current listings as sent without posting.
    pub(crate) seed_only: bool,
}

impl Config {
    pub(crate) fn from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Builds a config from `lookup` (an environment variable getter).
    /// Unset and blank values are treated alike, so an empty
    /// `Environment=RENT_IE_URL=` line disables a source instead of failing it.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let value = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let flag = |name: &str| {
            value(name).is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            })
        };

        let dry_run = flag("DRY_RUN");
        let seed_only = flag("SEED_SENT_IDS");
        let webhook_url = value("SLACK_WEBHOOK_URL");
        // Dry runs and seeding never post, so they don't need a webhook.
        if webhook_url.is_none() && !dry_run && !seed_only {
            bail!("SLACK_WEBHOOK_URL is not set");
        }

        let sources = Sources {
            rent_ie_url: value("RENT_IE_URL"),
            daft_ie_request: value("DAFT_IE_REQUEST"),
            property_ie_url: value("PROPERTY_IE_URL"),
        };
        if sources.rent_ie_url.is_none()
            && sources.daft_ie_request.is_none()
            && sources.property_ie_url.is_none()
        {
            bail!("Set RENT_IE_URL, DAFT_IE_REQUEST, and/or PROPERTY_IE_URL");
        }

        let max_beds = value("MAX_BEDS")
            .map(|value| value.parse::<u32>())
            .transpose()
            .context("Invalid MAX_BEDS value")?;
        let places = value(places::ENV_VAR)
            .map(|value| places::parse(&value))
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            sources,
            webhook_url,
            ors_api_key: value("OPENROUTESERVICE_API_KEY"),
            text_report_dir: value("TEXT_REPORT_DIR"),
            text_report_include_all: flag("TEXT_REPORT_INCLUDE_ALL"),
            max_beds,
            places,
            dry_run,
            seed_only,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use anyhow::Result;

    use super::Config;

    fn config(vars: &[(&str, &str)]) -> Result<Config> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        Config::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn reads_and_trims_settings() -> Result<()> {
        let config = config(&[
            ("SLACK_WEBHOOK_URL", " https://hooks.example/x "),
            ("RENT_IE_URL", "https://www.rent.ie/x"),
            ("DAFT_IE_REQUEST", "   "),
            ("MAX_BEDS", "4"),
            ("TEXT_REPORT_INCLUDE_ALL", "Yes"),
            (
                "PLACES_OF_INTEREST",
                r#"[{"name":"Work","latitude":52.2,"longitude":-7.1}]"#,
            ),
        ])?;

        assert_eq!(
            config.webhook_url.as_deref(),
            Some("https://hooks.example/x")
        );
        assert!(config.sources.rent_ie_url.is_some());
        assert!(config.sources.daft_ie_request.is_none());
        assert_eq!(config.max_beds, Some(4));
        assert!(config.text_report_include_all);
        assert_eq!(config.places.len(), 1);
        assert!(!config.dry_run && !config.seed_only);
        Ok(())
    }

    #[test]
    fn requires_a_webhook_unless_not_posting() -> Result<()> {
        let source = ("RENT_IE_URL", "https://www.rent.ie/x");
        assert!(config(&[source]).is_err());
        assert!(config(&[source, ("SLACK_WEBHOOK_URL", "")]).is_err());
        assert!(config(&[source, ("DRY_RUN", "1")])?.dry_run);
        assert!(config(&[source, ("SEED_SENT_IDS", "true")])?.seed_only);
        Ok(())
    }

    #[test]
    fn rejects_missing_sources_and_bad_values() {
        let webhook = ("SLACK_WEBHOOK_URL", "https://hooks.example/x");
        let source = ("PROPERTY_IE_URL", "https://www.property.ie/x");
        assert!(config(&[webhook]).is_err());
        assert!(config(&[webhook, ("RENT_IE_URL", " ")]).is_err());
        assert!(config(&[webhook, source, ("MAX_BEDS", "four")]).is_err());
        assert!(config(&[webhook, source, ("PLACES_OF_INTEREST", "[")]).is_err());
    }
}
